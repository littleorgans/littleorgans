#![allow(dead_code)]

use std::fmt::Debug;

pub fn assert_ordered_subsequence<T>(items: &[T], expected: &[T])
where
    T: Debug + PartialEq,
{
    let mut items_iter = items.iter();
    for expected_item in expected {
        assert!(
            items_iter.any(|item| item == expected_item),
            "missing ordered item {expected_item:?} in {items:?}"
        );
    }
}

pub trait OrPanic<T> {
    fn or_panic(self, context: &str) -> T;
}

impl<T, E: Debug> OrPanic<T> for Result<T, E> {
    fn or_panic(self, context: &str) -> T {
        match self {
            Ok(value) => value,
            Err(error) => panic!("{context}: {error:?}"),
        }
    }
}

impl<T> OrPanic<T> for Option<T> {
    fn or_panic(self, context: &str) -> T {
        match self {
            Some(value) => value,
            None => panic!("{context}"),
        }
    }
}

pub trait ErrOrPanic<E> {
    fn err_or_panic(self, context: &str) -> E;
}

impl<T, E> ErrOrPanic<E> for Result<T, E> {
    fn err_or_panic(self, context: &str) -> E {
        match self {
            Ok(_) => panic!("{context}: expected error"),
            Err(error) => error,
        }
    }
}
