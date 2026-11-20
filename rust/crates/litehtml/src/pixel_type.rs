//! `pixel_type.h`: `pixel_t`, a float whose comparisons are fuzzy. Two values within 1e-4 of each other are
//! equal, and one is less than another only when it is also more than 1e-4 below it; `<=` is "less or
//! equal" in that sense. NaN equals nothing and orders with nothing, and so does an infinity with itself
//! (their difference is NaN). Arithmetic is plain `f32` arithmetic.

use std::cmp::Ordering;
use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};

use crate::cxx;

/// `litehtml::pixel_t` (`pixel_float_t`).
#[derive(Clone, Copy, Debug, Default)]
pub struct Px(pub f32);

const EPSILON: f32 = 0.0001;

impl Px {
    pub const fn new(value: f32) -> Px {
        Px(value)
    }

    /// `pixel_float_t(int)`.
    pub fn from_int(value: i32) -> Px {
        Px(value as f32)
    }

    /// `value()` and `operator float()`.
    pub const fn value(self) -> f32 {
        self.0
    }

    /// `operator int()`: `static_cast<int>` of the value.
    pub fn to_int(self) -> i32 {
        cxx::f32_to_int(self.0)
    }

    /// `std::max(self, other)`: `self < other ? other : self`, with the fuzzy `<`.
    pub fn max(self, other: Px) -> Px {
        if self < other { other } else { self }
    }

    /// `std::min(self, other)`: `other < self ? other : self`, with the fuzzy `<`.
    pub fn min(self, other: Px) -> Px {
        if other < self { other } else { self }
    }

    /// Prefix `++`.
    pub fn inc(&mut self) -> Px {
        self.0 += 1.0;
        *self
    }

    /// Prefix `--`.
    pub fn dec(&mut self) -> Px {
        self.0 -= 1.0;
        *self
    }
}

impl From<f32> for Px {
    fn from(value: f32) -> Px {
        Px(value)
    }
}

impl From<i32> for Px {
    fn from(value: i32) -> Px {
        Px::from_int(value)
    }
}

impl From<Px> for f32 {
    fn from(value: Px) -> f32 {
        value.0
    }
}

impl PartialEq for Px {
    fn eq(&self, other: &Px) -> bool {
        (self.0 - other.0).abs() < EPSILON
    }
}

impl PartialOrd for Px {
    fn partial_cmp(&self, other: &Px) -> Option<Ordering> {
        if self == other {
            Some(Ordering::Equal)
        } else if self < other {
            Some(Ordering::Less)
        } else if self > other {
            Some(Ordering::Greater)
        } else {
            None
        }
    }

    fn lt(&self, other: &Px) -> bool {
        self.0 < other.0 && (self.0 - other.0).abs() >= EPSILON
    }

    fn gt(&self, other: &Px) -> bool {
        self.0 > other.0 && (self.0 - other.0).abs() >= EPSILON
    }

    fn le(&self, other: &Px) -> bool {
        self.lt(other) || self.eq(other)
    }

    fn ge(&self, other: &Px) -> bool {
        self.gt(other) || self.eq(other)
    }
}

impl Neg for Px {
    type Output = Px;
    fn neg(self) -> Px {
        Px(-self.0)
    }
}

macro_rules! arithmetic {
    ($trait:ident, $method:ident, $assign:ident, $assign_method:ident, $op:tt) => {
        impl $trait<Px> for Px {
            type Output = Px;
            fn $method(self, other: Px) -> Px {
                Px(self.0 $op other.0)
            }
        }
        impl $trait<f32> for Px {
            type Output = Px;
            fn $method(self, other: f32) -> Px {
                Px(self.0 $op other)
            }
        }
        impl $trait<i32> for Px {
            type Output = Px;
            fn $method(self, other: i32) -> Px {
                Px(self.0 $op other as f32)
            }
        }
        impl $trait<Px> for f32 {
            type Output = Px;
            fn $method(self, other: Px) -> Px {
                Px(self $op other.0)
            }
        }
        impl $trait<Px> for i32 {
            type Output = Px;
            fn $method(self, other: Px) -> Px {
                Px(self as f32 $op other.0)
            }
        }
        impl $assign<Px> for Px {
            fn $assign_method(&mut self, other: Px) {
                self.0 = self.0 $op other.0;
            }
        }
        impl $assign<f32> for Px {
            fn $assign_method(&mut self, other: f32) {
                self.0 = self.0 $op other;
            }
        }
        impl $assign<i32> for Px {
            fn $assign_method(&mut self, other: i32) {
                self.0 = self.0 $op other as f32;
            }
        }
    };
}

arithmetic!(Add, add, AddAssign, add_assign, +);
arithmetic!(Sub, sub, SubAssign, sub_assign, -);
arithmetic!(Mul, mul, MulAssign, mul_assign, *);
arithmetic!(Div, div, DivAssign, div_assign, /);
