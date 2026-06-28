/// A simple enum to represent either a left or right value. Similar to
/// [`Result`], but without the semantic meaning of "Ok" and "Err".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Either<L, R> {
    Left(L),
    Right(R),
}

impl<L, R> Either<L, R> {
    pub fn is_left(&self) -> bool {
        matches!(self, Self::Left(_))
    }

    pub fn is_right(&self) -> bool {
        matches!(self, Self::Right(_))
    }

    pub fn left(&self) -> Option<&L> {
        match self {
            Self::Left(l) => Some(l),
            Self::Right(_) => None,
        }
    }

    pub fn right(&self) -> Option<&R> {
        match self {
            Self::Left(_) => None,
            Self::Right(r) => Some(r),
        }
    }

    pub fn left_mut(&mut self) -> Option<&mut L> {
        match self {
            Self::Left(l) => Some(l),
            Self::Right(_) => None,
        }
    }

    pub fn right_mut(&mut self) -> Option<&mut R> {
        match self {
            Self::Left(_) => None,
            Self::Right(r) => Some(r),
        }
    }

    pub fn into_left(self) -> Result<L, R> {
        match self {
            Self::Left(l) => Ok(l),
            Self::Right(r) => Err(r),
        }
    }

    pub fn into_right(self) -> Result<R, L> {
        match self {
            Self::Left(l) => Err(l),
            Self::Right(r) => Ok(r),
        }
    }
}
