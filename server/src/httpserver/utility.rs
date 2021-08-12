use rand::{rngs::StdRng, Rng, SeedableRng};

pub fn make_random_cookie_key() -> [u8; 32] {
	let mut cookie_private_key = [0u8; 32];
	let mut rng = StdRng::from_entropy();
	rng.fill(&mut cookie_private_key[..]);
	cookie_private_key
}
