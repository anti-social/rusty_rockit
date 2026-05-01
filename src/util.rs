use core::num::NonZero;
use core::ops::Mul;

pub(crate) fn align2(size: u32) -> u32 {
    (size + 1) & !1
}

pub struct RatioU32 {
    numer: u32,
    denom: NonZero<u32>,
}

impl RatioU32 {
    pub const fn new(numer: u32, denom: u32) -> Self {
        let Some(denom) = NonZero::new(denom) else {
            panic!("Denominator cannot be zero");
        };
        Self { numer, denom }
    }

    pub fn ceil(&self) -> u32 {
        if self.numer % self.denom == 0 {
            self.numer / self.denom
        } else {
            self.numer / self.denom + 1
        }
    }
}

impl From<u32> for RatioU32 {
    fn from(numer: u32) -> Self {
        Self::new(numer, 1)
    }
}

impl Mul<u32> for RatioU32 {
    type Output = RatioU32;

    fn mul(self, rhs: u32) -> Self::Output {
        let gcd = gcd(rhs, self.denom.get());
        RatioU32::new(self.numer * (rhs / gcd), self.denom.get() / gcd)
    }
}

// Euclid's GCD
fn gcd(x: u32, y: u32) -> u32 {
    let mut x = x;
    let mut y = y;
    while y != 0 {
        let t = y;
        y = x % y;
        x = t;
    }
    x
}
