#![doc = include_str!("../README.md")]

#[cfg(all(feature = "agent", not(target_arch = "wasm32")))]
pub mod agent;
pub mod browser;
pub mod comb;
pub mod drop;
pub mod form;
pub mod key;
#[cfg(feature = "markdown")]
pub mod markdown;
pub mod meter;
pub mod panel;
pub mod readout;
#[cfg(not(target_arch = "wasm32"))]
pub mod revision;
#[cfg(all(feature = "speech", not(target_arch = "wasm32")))]
pub mod speech;
pub mod table;
pub mod text;
pub mod theme;
pub mod toast;
pub mod trace;
#[cfg(all(feature = "update", not(target_arch = "wasm32")))]
pub mod update;
#[cfg(feature = "viewer3d")]
pub mod viewer3d;
pub mod voice;

pub use theme::install;

pub mod prelude {
    pub use crate::browser::{Browser, Picked};
    pub use crate::comb::{Channel, Comb};
    pub use crate::form::{
        choice, clipboard_menu, field, field_then, footer, modal_title, prose, row, row_help,
        secret, switch,
    };
    pub use crate::key::{KEY_H, Key, Look, busy};
    pub use crate::meter::{FADER_H, Fader, Threshold, VU_H, bar, progress, vu};
    pub use crate::panel::{
        card, chip, exit_note, heading, lamp, section, segments, stage_rail, status, tabs, toggle,
    };
    pub use crate::readout::{LABEL_W, READOUT_W, hero, reading, readout, readouts};
    pub use crate::table::{ROW_H, Table, cell};
    pub use crate::text::{Line, action, help, hint, legend, legend_job, note, value};
    pub use crate::theme::{
        self, BAND, CHASSIS, ETCH, FAULT, LEGEND, OK, PANEL, READOUT, READOUT_DIM, SAFE, TRACE,
        VALUE, WARN, WELL, figure, install, legend_font, mono,
    };
    pub use crate::toast::{Tone, toast};
    pub use crate::trace::{Scale, Trace};
}

#[cfg(test)]
pub(crate) trait RunFrame {
    fn run_frame(&self, input: egui::RawInput, f: impl FnMut(&mut egui::Ui)) -> egui::FullOutput;
}

#[cfg(test)]
impl RunFrame for egui::Context {
    fn run_frame(&self, input: egui::RawInput, f: impl FnMut(&mut egui::Ui)) -> egui::FullOutput {
        let mut out = self.run_ui(input, f);
        out.textures_delta.clear();
        out
    }
}
