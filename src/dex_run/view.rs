use crate::apk::ApkArchive;
use crate::binary_xml::{self, XmlEvent};

use super::*;

const DEFAULT_VIEW_TAG_ID: i32 = -1;
const PAINT_FILTER_BITMAP_FLAG: i32 = 0x02;

mod click;
mod dynamic_children;
mod generic_motion;
mod graphics;
mod helpers;
pub(in crate::dex_run) use helpers::*;
mod inflate;
mod input;
mod lifecycle;
mod observers;
mod text;
mod view_group;
mod widgets;
