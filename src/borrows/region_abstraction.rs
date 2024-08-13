

use rustc_interface::{
    data_structures::{
        fx::{FxHashSet},
    },
    middle::mir::{Location},
};


use crate::{
    rustc_interface,
    utils::{Place},
};

use super::{
    domain::{
        AbstractionBlockEdge, AbstractionTarget, AbstractionType, Latest, MaybeOldPlace,
    },
};

#[derive(PartialEq, Eq, Clone, Debug, Hash)]
pub struct RegionAbstraction<'tcx> {
    pub abstraction_type: AbstractionType<'tcx>,
}

impl<'tcx> RegionAbstraction<'tcx> {
    pub fn make_place_old(&mut self, place: Place<'tcx>, latest: &Latest<'tcx>) {
        self.abstraction_type.make_place_old(place, latest);
    }

    pub fn new(abstraction_type: AbstractionType<'tcx>) -> Self {
        Self { abstraction_type }
    }

    pub fn location(&self) -> Location {
        self.abstraction_type.location()
    }

    pub fn inputs(&self) -> Vec<&AbstractionTarget<'tcx>> {
        self.abstraction_type.inputs()
    }

    pub fn outputs(&self) -> Vec<&AbstractionTarget<'tcx>> {
        self.abstraction_type.outputs()
    }

    pub fn blocks(&self, place: &MaybeOldPlace<'tcx>) -> bool {
        self.abstraction_type.blocks(place)
    }

    pub fn blocks_places(&self) -> FxHashSet<MaybeOldPlace<'tcx>> {
        self.abstraction_type.blocks_places()
    }

    pub fn blocked_by_places(&self) -> FxHashSet<MaybeOldPlace<'tcx>> {
        self.abstraction_type.blocker_places()
    }

    pub fn edges(&self) -> impl Iterator<Item = &AbstractionBlockEdge<'tcx>> {
        match &self.abstraction_type {
            AbstractionType::FunctionCall { edges, .. } => edges.iter().map(|(_, edge)| edge),
        }
    }
}