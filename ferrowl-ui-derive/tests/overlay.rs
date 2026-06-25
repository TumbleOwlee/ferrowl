//! Tests for `#[derive(Overlay)]`: structural helpers + common-key routing.

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::traits::{OverlayKeys, OverlayRoute};
use ferrowl_ui_derive::Overlay;

// --- Payloads --------------------------------------------------------------

#[derive(Debug, PartialEq)]
struct Editor {
    step: i32,
}
impl OverlayKeys for Editor {
    fn focus_cycle(&mut self, forward: bool) {
        self.step += if forward { 1 } else { -1 };
    }
}

#[derive(Debug, PartialEq)]
struct Setup {
    step: i32,
}
impl OverlayKeys for Setup {
    fn focus_cycle(&mut self, forward: bool) {
        self.step += if forward { 10 } else { -10 };
    }
}

#[derive(Debug, PartialEq)]
struct Confirm; // esc_close only, no focus cycling

#[derive(Debug, PartialEq)]
struct Picker; // untagged: never handled by common routing

#[derive(Debug, PartialEq, Overlay)]
enum Overlay {
    #[overlay(none)]
    None,
    #[overlay(esc_close, focus_cycle)]
    Edit(Editor),
    #[overlay(focus_cycle)]
    SetupV(Setup),
    #[overlay(esc_close)]
    Conf(Confirm),
    Plain(Picker),
}

const NONE: KeyModifiers = KeyModifiers::NONE;

fn route(o: &mut Overlay, code: KeyCode) -> OverlayRoute {
    o.route_keys(NONE, code)
}

// --- Structural ------------------------------------------------------------

#[test]
fn ut_is_active() {
    assert!(!Overlay::None.is_active());
    assert!(Overlay::Edit(Editor { step: 0 }).is_active());
    assert!(Overlay::Plain(Picker).is_active());
}

#[test]
fn ut_take_leaves_none() {
    let mut o = Overlay::Edit(Editor { step: 3 });
    let taken = o.take();
    assert_eq!(taken, Overlay::Edit(Editor { step: 3 }));
    assert_eq!(o, Overlay::None);
}

#[test]
fn ut_close_resets_to_none() {
    let mut o = Overlay::SetupV(Setup { step: 1 });
    o.close();
    assert_eq!(o, Overlay::None);
}

// --- esc_close -------------------------------------------------------------

#[test]
fn ut_esc_closes_esc_close_variant() {
    let mut o = Overlay::Edit(Editor { step: 0 });
    assert_eq!(route(&mut o, KeyCode::Esc), OverlayRoute::Closed);
    assert_eq!(o, Overlay::None);

    let mut o = Overlay::Conf(Confirm);
    assert_eq!(route(&mut o, KeyCode::Esc), OverlayRoute::Closed);
    assert_eq!(o, Overlay::None);
}

#[test]
fn ut_esc_unhandled_without_esc_close() {
    // SetupV is focus_cycle only; Plain is untagged. Neither closes on Esc.
    let mut o = Overlay::SetupV(Setup { step: 0 });
    assert_eq!(route(&mut o, KeyCode::Esc), OverlayRoute::Unhandled);
    assert!(o.is_active());

    let mut o = Overlay::Plain(Picker);
    assert_eq!(route(&mut o, KeyCode::Esc), OverlayRoute::Unhandled);
    assert!(o.is_active());
}

// --- focus_cycle -----------------------------------------------------------

#[test]
fn ut_tab_cycles_focus_forward() {
    let mut o = Overlay::Edit(Editor { step: 0 });
    assert_eq!(route(&mut o, KeyCode::Tab), OverlayRoute::Cycled);
    assert_eq!(o, Overlay::Edit(Editor { step: 1 }));
}

#[test]
fn ut_backtab_cycles_focus_backward() {
    let mut o = Overlay::SetupV(Setup { step: 0 });
    assert_eq!(o.route_keys(NONE, KeyCode::BackTab), OverlayRoute::Cycled);
    assert_eq!(o, Overlay::SetupV(Setup { step: -10 }));
    // Shift+BackTab also cycles.
    assert_eq!(
        o.route_keys(KeyModifiers::SHIFT, KeyCode::BackTab),
        OverlayRoute::Cycled
    );
    assert_eq!(o, Overlay::SetupV(Setup { step: -20 }));
}

#[test]
fn ut_tab_unhandled_without_focus_cycle() {
    let mut o = Overlay::Conf(Confirm);
    assert_eq!(route(&mut o, KeyCode::Tab), OverlayRoute::Unhandled);
}

// --- fall-through ----------------------------------------------------------

#[test]
fn ut_other_keys_unhandled() {
    let mut o = Overlay::Edit(Editor { step: 0 });
    assert_eq!(route(&mut o, KeyCode::Enter), OverlayRoute::Unhandled);
    assert_eq!(route(&mut o, KeyCode::Char('x')), OverlayRoute::Unhandled);
    // Untouched.
    assert_eq!(o, Overlay::Edit(Editor { step: 0 }));
}

#[test]
fn ut_none_is_unhandled() {
    let mut o = Overlay::None;
    assert_eq!(route(&mut o, KeyCode::Esc), OverlayRoute::Unhandled);
    assert_eq!(route(&mut o, KeyCode::Tab), OverlayRoute::Unhandled);
}
