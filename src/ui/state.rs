//! State management for the Voice Studio UI
//!
//! This module contains the data model, custom events, and synchronization logic
//! for the UI state.

use crate::macro_controller;
use crate::version::{VersionEvent, VersionUiState};
use crate::VoiceParams;
use nih_plug::prelude::{GuiContext, ParamSetter};
use nih_plug_vizia::vizia::prelude::*;
use std::sync::Arc;

#[derive(Lens, Clone)]
pub struct VoiceStudioData {
    pub params: Arc<VoiceParams>,
    pub advanced_tab: AdvancedTab,
    pub version_info: VersionUiState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Data)]
pub enum AdvancedTab {
    CleanRepair,
    ShapePolish,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AdvancedTabEvent {
    SetTab(AdvancedTab),
}

impl Model for VoiceStudioData {
    #[allow(unused_variables)]
    fn event(&mut self, cx: &mut EventContext, event: &mut Event) {
        event.map(|advanced_tab_event, _| match advanced_tab_event {
            AdvancedTabEvent::SetTab(tab) => self.advanced_tab = *tab,
        });

        event.map(|version_event, _| match version_event {
            VersionEvent::Update(info) => {
                self.version_info = info.clone();
                cx.needs_redraw();
            }
        });
    }
}

// Sync functions
pub fn sync_advanced_from_macros(params: &Arc<VoiceParams>, gui: Arc<dyn GuiContext>) {
    let setter = ParamSetter::new(gui.as_ref());
    macro_controller::apply_simple_macros(params.as_ref(), &setter);
}

pub fn set_macro_mode(params: &Arc<VoiceParams>, gui_context: &Arc<dyn GuiContext>, enabled: bool) {
    let setter = ParamSetter::new(gui_context.as_ref());
    setter.begin_set_parameter(&params.macro_mode);
    setter.set_parameter(&params.macro_mode, enabled);
    setter.end_set_parameter(&params.macro_mode);
}
