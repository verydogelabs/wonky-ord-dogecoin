use crate::sat::Sat;
use crate::sat_point::SatPoint;
use super::*;

#[derive(Boilerplate)]
pub(crate) struct RareTxt(pub(crate) Vec<(Sat, SatPoint)>);
