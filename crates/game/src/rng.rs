//! PCG32 (XSH RR). Seeded by the shell, so every run is reproducible.

#[derive(Clone, Debug)]
pub(crate) struct Rng {
    state: u64,
    inc: u64,
}

const MUL: u64 = 6364136223846793005;

impl Rng {
    pub fn new(seed: u64) -> Rng {
        let mut r = Rng {
            state: 0,
            inc: (0xda3e_39cb_94b9_5bdb << 1) | 1,
        };
        r.next_u32();
        r.state = r.state.wrapping_add(seed);
        r.next_u32();
        r
    }

    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(MUL).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    pub fn next_u64(&mut self) -> u64 {
        (u64::from(self.next_u32()) << 32) | u64::from(self.next_u32())
    }

    /// Uniform in `[0, 1)`.
    pub fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (1.0 / (1u32 << 24) as f32)
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }

    /// Uniform in `0..n`. `n` must be positive.
    pub fn below(&mut self, n: usize) -> usize {
        // Lemire's multiply-shift. The bias is under 2^-32 for our small n.
        ((u64::from(self.next_u32()) * n as u64) >> 32) as usize
    }

    pub fn chance(&mut self, p: f32) -> bool {
        self.unit() < p
    }
}

#[cfg(test)]
mod tests {
    use super::Rng;

    #[test]
    fn same_seed_same_stream_and_below_stays_in_range() {
        let (mut a, mut b) = (Rng::new(7), Rng::new(7));
        let xs: Vec<u32> = (0..64).map(|_| a.next_u32()).collect();
        let ys: Vec<u32> = (0..64).map(|_| b.next_u32()).collect();
        assert_eq!(xs, ys);
        assert_ne!(Rng::new(8).next_u32(), Rng::new(7).next_u32());
        let mut r = Rng::new(1);
        let mut seen = [false; 5];
        for _ in 0..1000 {
            seen[r.below(5)] = true;
            assert!((0.0..1.0).contains(&r.unit()));
        }
        assert!(seen.iter().all(|s| *s), "every bucket is reachable");
    }
}
