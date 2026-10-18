use getset::Getters;

use crate::meta::{Spanned, SpannedItem};

#[derive(Getters, Debug, Clone, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub struct ParseError {
    #[get = "pub"]
    pub reason: ParseErrorReason,
}

impl ParseError {
    pub fn mismatch(expected: impl Into<String>, actual: Spanned<impl Into<String>>) -> ParseError {
        let Spanned { span, item } = actual;

        ParseError {
            reason: ParseErrorReason::Mismatch {
                expected: expected.into(),
                actual: item.into().spanned(span),
            },
        }
    }

    pub fn argument_error(command: Spanned<impl Into<String>>, kind: ArgumentError) -> ParseError {
        ParseError {
            reason: ParseErrorReason::ArgumentError {
                command: command.item.into().spanned(command.span),
                error: kind,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub enum ParseErrorReason {
    Mismatch {
        expected: String,
        actual: Spanned<String>,
    },
    ArgumentError {
        command: Spanned<String>,
        error: ArgumentError,
    },
}

/// ArgumentError describes various ways that the parser could fail because of unexpected arguments.
/// These errors correspond to problems that could be identified during expansion based on the
/// signature of a command.
#[derive(Debug, Eq, PartialEq, Clone, Ord, Hash, PartialOrd)]
pub enum ArgumentError {
    /// The command specified a mandatory positional argument, but it was missing.
    MissingMandatoryPositional {
        name: Option<String>,
        positional_index: usize,
    },
    /// A flag was found, and it should have been followed by a value, but no value was found.
    MissingValueForName {
        /// The name of the flag/option
        name: Spanned<String>,
        /// The index of the missing argument for the option,
        /// e.g. if the flag was -D arg1 arg2, then missing_arg_index=0 corresponds to arg1
        missing_arg_index: usize,
    },
    /// An argument was found, but the command does not recognize it.
    UnexpectedArgument(Spanned<String>),
    /// An flag was found, but the command does not recognize it.
    UnexpectedFlag(Spanned<String>),
}
