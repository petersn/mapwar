#[derive(Debug, Clone)]
pub struct Rng {
  state: u64,
}

impl Rng {
  pub fn new() -> Self {
    Self {
      state: rand::random(),
    }
  }

  pub fn new_from_seed(seed: u64) -> Self {
    Self { state: seed }
  }

  pub fn generate(&mut self) -> u64 {
    self.state += 1;
    const MULT: u64 = 0x243f6a8885a308d3;
    let mut x = self.state;
    for _ in 0..3 {
      x = x.wrapping_mul(MULT);
      x ^= x >> 37;
    }
    x = x.wrapping_mul(MULT);
    x
  }
}
