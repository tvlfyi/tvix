//! Macros that generate proptest test suites checking laws of stdlib traits

/// Generate a suite of tests to check the laws of the [`Eq`] impl for the given type
macro_rules! eq_laws {
    ($ty: ty) => {
        eq_laws!(
            #[strategy(::proptest::arbitrary::any::<$ty>())]
            $ty
        );
    };
    (#[$meta: meta] $ty: ty) => {
        #[allow(clippy::eq_op)]
        mod eq {
            use test_strategy::proptest;

            use super::*;

            #[proptest]
            fn reflexive(#[$meta] x: $ty) {
                assert!(x == x);
            }

            #[proptest]
            fn symmetric(#[$meta] x: $ty, #[$meta] y: $ty) {
                assert_eq!(x == y, y == x);
            }

            #[proptest]
            fn transitive(#[$meta] x: $ty, #[$meta] y: $ty, #[$meta] z: $ty) {
                if x == y && y == z {
                    assert!(x == z);
                }
            }
        }
    };
}

/// Generate a suite of tests to check the laws of the [`Ord`] impl for the given type
macro_rules! ord_laws {
    ($ty: ty) => {
        ord_laws!(
            #[strategy(::proptest::arbitrary::any::<$ty>())]
            $ty
        );
    };
    (#[$meta: meta] $ty: ty) => {
        mod ord {
            use test_strategy::proptest;

            use super::*;

            #[proptest]
            fn partial_cmp_matches_cmp(#[$meta] x: $ty, #[$meta] y: $ty) {
                assert_eq!(x.partial_cmp(&y), Some(x.cmp(&y)));
            }

            #[proptest]
            fn dual(#[$meta] x: $ty, #[$meta] y: $ty) {
                if x < y {
                    assert!(y > x);
                }
                if y < x {
                    assert!(x > y);
                }
            }

            #[proptest]
            fn le_transitive(#[$meta] x: $ty, #[$meta] y: $ty, #[$meta] z: $ty) {
                if x < y && y < z {
                    assert!(x < z)
                }
            }

            #[proptest]
            fn gt_transitive(#[$meta] x: $ty, #[$meta] y: $ty, #[$meta] z: $ty) {
                if x > y && y > z {
                    assert!(x > z)
                }
            }

            #[proptest]
            fn trichotomy(#[$meta] x: $ty, #[$meta] y: $ty) {
                let less = x < y;
                let greater = x > y;
                let eq = x == y;

                if less {
                    assert!(!greater);
                    assert!(!eq);
                }

                if greater {
                    assert!(!less);
                    assert!(!eq);
                }

                if eq {
                    assert!(!less);
                    assert!(!greater);
                }
            }
        }
    };
}

/// Generate a test to check the laws of the [`Hash`] impl for the given type
macro_rules! hash_laws {
    ($ty: ty) => {
        hash_laws!(
            #[strategy(::proptest::arbitrary::any::<$ty>())]
            $ty
        );
    };
    (#[$meta: meta] $ty: ty) => {
        mod hash {
            use test_strategy::proptest;

            use super::*;

            #[proptest]
            fn matches_eq(#[$meta] x: $ty, #[$meta] y: $ty) {
                let hash = |x: &$ty| {
                    use std::hash::Hasher;

                    let mut hasher = ::std::collections::hash_map::DefaultHasher::new();
                    x.hash(&mut hasher);
                    hasher.finish()
                };

                if x == y {
                    assert_eq!(hash(&x), hash(&y));
                }
            }
        }
    };
}

pub(crate) use eq_laws;
pub(crate) use hash_laws;
pub(crate) use ord_laws;
