

fn main() {
	let mut x = 1;
	x += 1;

	let y = &mut x;

	*y = 0;

	assert!(x == 0);	
}
