use crate::{
    borrows::borrows_state::BorrowsState,
    borrows::domain::Borrow,
    free_pcs::{CapabilityKind, CapabilityLocal, CapabilitySummary},
    rustc_interface,
    utils::{Place, PlaceRepacker},
};
use serde_derive::Serialize;
use std::{
    collections::{HashSet, VecDeque},
    fs::File,
    io::{self, Write},
    rc::Rc,
};

use rustc_interface::{
    borrowck::{
        borrow_set::BorrowSet,
        consumers::{
            calculate_borrows_out_of_scope_at_location, BorrowIndex, Borrows, LocationTable,
            PoloniusInput, PoloniusOutput, RegionInferenceContext,
        },
    },
    data_structures::fx::{FxHashMap, FxIndexMap},
    dataflow::{Analysis, ResultsCursor},
    index::IndexVec,
    middle::{
        mir::{
            self, BinOp, Body, Local, Location, Operand, PlaceElem, Promoted, Rvalue, Statement,
            TerminatorKind, UnwindAction, VarDebugInfo, RETURN_PLACE,
        },
        ty::{self, GenericArgsRef, ParamEnv, RegionVid, TyCtxt},
    },
};

#[derive(Serialize)]
struct MirGraph {
    nodes: Vec<MirNode>,
    edges: Vec<MirEdge>,
}

#[derive(Serialize)]
struct MirNode {
    id: String,
    block: usize,
    stmts: Vec<String>,
    terminator: String,
}

#[derive(Serialize)]
struct MirEdge {
    source: String,
    target: String,
    label: String,
}

fn format_bin_op(op: &BinOp) -> String {
    match op {
        BinOp::Add => "+".to_string(),
        BinOp::Sub => "-".to_string(),
        BinOp::Mul => "*".to_string(),
        BinOp::Div => "/".to_string(),
        BinOp::Rem => "%".to_string(),
        BinOp::AddUnchecked => todo!(),
        BinOp::SubUnchecked => todo!(),
        BinOp::MulUnchecked => todo!(),
        BinOp::BitXor => todo!(),
        BinOp::BitAnd => "&".to_string(),
        BinOp::BitOr => todo!(),
        BinOp::Shl => "<<".to_string(),
        BinOp::ShlUnchecked => "<<".to_string(),
        BinOp::Shr => ">>".to_string(),
        BinOp::ShrUnchecked => ">>".to_string(),
        BinOp::Eq => "==".to_string(),
        BinOp::Lt => "<".to_string(),
        BinOp::Le => "<=".to_string(),
        BinOp::Ne => "!=".to_string(),
        BinOp::Ge => ">=".to_string(),
        BinOp::Gt => ">".to_string(),
        BinOp::Offset => todo!(),
    }
}

fn format_local<'tcx>(local: &Local, repacker: PlaceRepacker<'_, 'tcx>) -> String {
    let place: Place<'tcx> = (*local).into();
    place.to_short_string(repacker)
}

fn format_place<'tcx>(place: &mir::Place<'tcx>, repacker: PlaceRepacker<'_, 'tcx>) -> String {
    let place: Place<'tcx> = (*place).into();
    place.to_short_string(repacker)
}

fn format_operand<'tcx>(operand: &Operand<'tcx>, repacker: PlaceRepacker<'_, 'tcx>) -> String {
    match operand {
        Operand::Copy(p) => format_place(p, repacker),
        Operand::Move(p) => format!("move {}", format_place(p, repacker)),
        Operand::Constant(c) => format!("{}", c),
    }
}

fn format_rvalue<'tcx>(rvalue: &Rvalue<'tcx>, repacker: PlaceRepacker<'_, 'tcx>) -> String {
    match rvalue {
        Rvalue::Use(operand) => format_operand(operand, repacker),
        Rvalue::Repeat(_, _) => todo!(),
        Rvalue::Ref(region, kind, place) => {
            let kind = match kind {
                mir::BorrowKind::Shared => "",
                mir::BorrowKind::Shallow => "",
                mir::BorrowKind::Mut { .. } => "mut",
            };
            format!("&{} {}", kind, format_place(place, repacker))
        }
        Rvalue::ThreadLocalRef(_) => todo!(),
        Rvalue::AddressOf(_, _) => todo!(),
        Rvalue::Len(_) => todo!(),
        Rvalue::Cast(_, _, _) => todo!(),
        Rvalue::BinaryOp(op, box (lhs, rhs)) | Rvalue::CheckedBinaryOp(op, box (lhs, rhs)) => {
            format!(
                "{} {} {}",
                format_operand(lhs, repacker),
                format_bin_op(op),
                format_operand(rhs, repacker)
            )
        }
        Rvalue::NullaryOp(_, _) => todo!(),
        Rvalue::UnaryOp(op, val) => {
            format!("{:?} {}", op, format_operand(val, repacker))
        }
        Rvalue::Discriminant(place) => format!("Discriminant({})", format_place(place, repacker)),
        Rvalue::Aggregate(kind, ops) => {
            format!(
                "Aggregate {:?} {}",
                kind,
                ops.iter()
                    .map(|op| format_operand(op, repacker))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
        Rvalue::ShallowInitBox(_, _) => todo!(),
        Rvalue::CopyForDeref(_) => todo!(),
    }
}
fn format_terminator<'tcx>(
    terminator: &TerminatorKind<'tcx>,
    repacker: PlaceRepacker<'_, 'tcx>,
) -> String {
    match terminator {
        TerminatorKind::Call {
            func,
            args,
            destination,
            target,
            unwind,
            call_source,
            fn_span,
        } => {
            format!(
                "{} = {}({})",
                format_place(destination, repacker),
                format_operand(func, repacker),
                args.iter()
                    .map(|arg| format_operand(arg, repacker))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
        _ => format!("{:?}", terminator),
    }
}

fn format_stmt<'tcx>(stmt: &Statement<'tcx>, repacker: PlaceRepacker<'_, 'tcx>) -> String {
    match &stmt.kind {
        mir::StatementKind::Assign(box (place, rvalue)) => {
            format!(
                "{} = {}",
                format_place(place, repacker),
                format_rvalue(rvalue, repacker)
            )
        }
        mir::StatementKind::FakeRead(box (_, place)) => {
            format!("FakeRead({})", format_place(place, repacker))
        }
        mir::StatementKind::SetDiscriminant {
            place,
            variant_index,
        } => todo!(),
        mir::StatementKind::Deinit(_) => todo!(),
        mir::StatementKind::StorageLive(local) => {
            format!("StorageLive({})", format_local(local, repacker))
        }
        mir::StatementKind::StorageDead(local) => {
            format!("StorageDead({})", format_local(local, repacker))
        }
        mir::StatementKind::Retag(_, _) => todo!(),
        mir::StatementKind::PlaceMention(place) => {
            format!("PlaceMention({})", format_place(place, repacker))
        }
        mir::StatementKind::AscribeUserType(_, _) => {
            format!("AscribeUserType(...)")
        }
        mir::StatementKind::Coverage(_) => todo!(),
        mir::StatementKind::Intrinsic(_) => todo!(),
        mir::StatementKind::ConstEvalCounter => todo!(),
        mir::StatementKind::Nop => todo!(),
    }
}

fn mk_mir_graph<'mir, 'tcx>(tcx: TyCtxt<'tcx>, body: &'mir Body<'tcx>) -> MirGraph {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    let repacker = PlaceRepacker::new(body, tcx);

    for (bb, data) in body.basic_blocks.iter_enumerated() {
        let stmts = data
            .statements
            .iter()
            .map(|stmt| format_stmt(stmt, repacker));

        let terminator = format_terminator(&data.terminator().kind, repacker);

        nodes.push(MirNode {
            id: format!("{:?}", bb),
            block: bb.as_usize(),
            stmts: stmts.collect(),
            terminator,
        });

        match &data.terminator().kind {
            TerminatorKind::Goto { target } => {
                edges.push(MirEdge {
                    source: format!("{:?}", bb),
                    target: format!("{:?}", target),
                    label: "goto".to_string(),
                });
            }
            TerminatorKind::SwitchInt { discr, targets } => {
                for (val, target) in targets.iter() {
                    edges.push(MirEdge {
                        source: format!("{:?}", bb),
                        target: format!("{:?}", target),
                        label: format!("{}", val),
                    });
                }
                edges.push(MirEdge {
                    source: format!("{:?}", bb),
                    target: format!("{:?}", targets.otherwise()),
                    label: "otherwise".to_string(),
                });
            }
            TerminatorKind::UnwindResume => {}
            TerminatorKind::UnwindTerminate(_) => todo!(),
            TerminatorKind::Return => {}
            TerminatorKind::Unreachable => {}
            TerminatorKind::Drop {
                place,
                target,
                unwind,
                replace,
            } => {
                edges.push(MirEdge {
                    source: format!("{:?}", bb),
                    target: format!("{:?}", target),
                    label: "drop".to_string(),
                });
            }
            TerminatorKind::Call {
                func,
                args,
                destination,
                target,
                unwind,
                call_source,
                fn_span,
            } => {
                if let Some(target) = target {
                    edges.push(MirEdge {
                        source: format!("{:?}", bb),
                        target: format!("{:?}", target),
                        label: "call".to_string(),
                    });
                    match unwind {
                        UnwindAction::Continue => todo!(),
                        UnwindAction::Unreachable => todo!(),
                        UnwindAction::Terminate(_) => todo!(),
                        UnwindAction::Cleanup(cleanup) => {
                            edges.push(MirEdge {
                                source: format!("{:?}", bb),
                                target: format!("{:?}", cleanup),
                                label: "unwind".to_string(),
                            });
                        }
                    }
                }
            }
            TerminatorKind::Assert {
                cond,
                expected,
                msg,
                target,
                unwind,
            } => {
                match unwind {
                    UnwindAction::Continue => todo!(),
                    UnwindAction::Unreachable => todo!(),
                    UnwindAction::Terminate(_) => todo!(),
                    UnwindAction::Cleanup(cleanup) => {
                        edges.push(MirEdge {
                            source: format!("{:?}", bb),
                            target: format!("{:?}", cleanup),
                            label: format!("unwind"),
                        });
                    }
                }
                edges.push(MirEdge {
                    source: format!("{:?}", bb),
                    target: format!("{:?}", target),
                    label: format!("success"),
                });
            }
            TerminatorKind::Yield {
                value,
                resume,
                resume_arg,
                drop,
            } => todo!(),
            TerminatorKind::GeneratorDrop => todo!(),
            TerminatorKind::FalseEdge {
                real_target,
                imaginary_target,
            } => {
                edges.push(MirEdge {
                    source: format!("{:?}", bb),
                    target: format!("{:?}", real_target),
                    label: "real".to_string(),
                });
            }
            TerminatorKind::FalseUnwind {
                real_target,
                unwind,
            } => {
                edges.push(MirEdge {
                    source: format!("{:?}", bb),
                    target: format!("{:?}", real_target),
                    label: "real".to_string(),
                });
            }
            TerminatorKind::InlineAsm {
                template,
                operands,
                options,
                line_spans,
                destination,
                unwind,
            } => todo!(),
        }
    }

    MirGraph { nodes, edges }
}
pub fn generate_json_from_mir<'mir, 'tcx>(
    path: &str,
    tcx: TyCtxt<'tcx>,
    body: &'mir Body<'tcx>,
) -> io::Result<()> {
    let mir_graph = mk_mir_graph(tcx, body);
    let mut file = File::create(path)?;
    serde_json::to_writer(&mut file, &mir_graph)?;
    Ok(())
}
