//! Macros that generate proptest test suites checking laws of stdlib traits

/// Generate a suite of tests to check the laws of the [`Eq`] impl for the given type
macro_rules! eq_laws {
    ($ty: ty) => {
        eq_laws!(
            #[strategy(::proptest::arbitrary::any::<$ty>())]
            $ty,
            Default::default()
        );
    };
    ($ty: ty, $config: expr) => {
        eq_laws!(
            #[strategy(::proptest::arbitrary::any::<$ty>())]
            $ty,
            $config
        );
    };
    (#[$meta: meta] $ty: ty, $config: expr) => {
        #[allow(clippy::eq_op)]
        mod eq {
            use test_strategy::proptest;

            use super::*;

            #[proptest($config)]
            fn reflexive(#[$meta] x: $ty) {
                assert!(x == x);
            }

            #[proptest($config)]
            fn symmetric(#[$meta] x: $ty, #[$meta] y: $ty) {
                assert_eq!(x == y, y == x);
            }

            #[proptest($config)]
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
            $ty,
            Default::default()
        );
    };
    ($ty: ty, $config: expr) => {
        ord_laws!(
            #[strategy(::proptest::arbitrary::any::<$ty>())]
            $ty,
            $config
        );
    };
    (#[$meta: meta] $ty: ty, $config: expr) => {
        mod ord {
            use test_strategy::proptest;

            use super::*;

            #[proptest($config)]
            fn partial_cmp_matches_cmp(#[$meta] x: $ty, #[$meta] y: $ty) {
                assert_eq!(x.partial_cmp(&y), Some(x.cmp(&y)));
            }

            #[proptest($config)]
            fn dual(#[$meta] x: $ty, #[$meta] y: $ty) {
                if x < y {
                    assert!(y > x);
                }
                if y < x {
                    assert!(x > y);
                }
            }

            #[proptest($config)]
            fn le_transitive(#[$meta] x: $ty, #[$meta] y: $ty, #[$meta] z: $ty) {
                if x < y && y < z {
                    assert!(x < z)
                }
            }

            #[proptest($config)]
            fn gt_transitive(#[$meta] x: $ty, #[$meta] y: $ty, #[$meta] z: $ty) {
                if x > y && y > z {
                    assert!(x > z)
                }
            }

            #[proptest($config)]
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
            $ty,
            Default::default()
        );
    };
    ($ty: ty, $config: expr) => {
        hash_laws!(
            #[strategy(::proptest::arbitrary::any::<$ty>())]
            $ty,
            $config
        );
    };
    (#[$meta: meta] $ty: ty, $config: expr) => {
        mod hash {
            use test_strategy::proptest;

            use super::*;

            #[proptest($config)]
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
