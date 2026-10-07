//! DOM-level integration tests for `martensite_access::web::WebA11yBridge`.
//!
//! These tests exercise the real DOM mirror — positioned elements,
//! editable-text projection, composites, roving tabindex, and opt-in
//! activation — and therefore only compile/run on
//! `wasm32-unknown-unknown` with a browser DOM. On any other target the
//! file compiles to an empty test crate so `cargo test` stays green on
//! native hosts.
//!
//! To execute: run under a wasm test driver that supplies a DOM, e.g.
//! `wasm-bindgen-test-runner`/headless browser, or drive them from a
//! `trunk`-served harness page. The logic-level invariants these tests
//! assume (spec/attr diffing, projection decisions, tabindex matrix)
//! are additionally covered by the unit tests inside `src/web.rs`,
//! which are type-checked by `cargo check --target wasm32-unknown-unknown
//! --all-targets`.

#![forbid(unsafe_code)]
#![cfg(all(target_arch = "wasm32", target_os = "unknown"))]

use accesskit::{Action, Node, NodeId, Role, TreeInfo, TreeUpdate};
use martensite_access::web::WebA11yBridge;
use wasm_bindgen::JsCast;

const ROOT: NodeId = NodeId(1);
const INPUT: NodeId = NodeId(2);
const PASSWORD: NodeId = NodeId(3);
const LISTBOX: NodeId = NodeId(4);
const OPTION_A: NodeId = NodeId(5);
const OPTION_B: NodeId = NodeId(6);

fn build_update() -> TreeUpdate {
    let mut root = Node::new(Role::RootWebArea);
    root.set_children([INPUT, PASSWORD, LISTBOX]);

    let mut input = Node::new(Role::TextInput);
    input.set_label("Name");
    input.set_value("Ada");
    input.set_bounds(accesskit::Rect::new(10.0, 10.0, 210.0, 40.0));
    input.add_action(Action::Focus);

    let mut password = Node::new(Role::PasswordInput);
    password.set_label("Secret");
    password.set_value("hunter2");
    password.add_action(Action::Focus);

    let mut listbox = Node::new(Role::ListBox);
    listbox.set_children([OPTION_A, OPTION_B]);
    listbox.set_active_descendant(OPTION_B);
    listbox.set_bounds(accesskit::Rect::new(10.0, 50.0, 210.0, 150.0));
    listbox.add_action(Action::Focus);

    let mut option_a = Node::new(Role::ListBoxOption);
    option_a.set_label("First");
    option_a.set_selected(false);
    option_a.add_action(Action::Focus);

    let mut option_b = Node::new(Role::ListBoxOption);
    option_b.set_label("Second");
    option_b.set_selected(true);
    option_b.add_action(Action::Focus);

    TreeUpdate {
        nodes: vec![
            (ROOT, root),
            (INPUT, input),
            (PASSWORD, password),
            (LISTBOX, listbox),
            (OPTION_A, option_a),
            (OPTION_B, option_b),
        ],
        tree: Some(TreeInfo::new(ROOT)),
        tree_id: accesskit::TreeId::ROOT,
        focus: ROOT,
    }
}

fn element(id: u64) -> Option<web_sys::HtmlElement> {
    web_sys::window()?
        .document()?
        .get_element_by_id(&format!("martensite-a11y-{id}"))?
        .dyn_into::<web_sys::HtmlElement>()
        .ok()
}

#[test]
fn mirror_stays_empty_until_enabled() {
    let mut bridge = WebA11yBridge::new().expect("bridge");
    assert!(!bridge.is_enabled());
    bridge.update(&build_update()).expect("update");
    assert_eq!(bridge.mirrored_count(), 0);
    assert!(element(1).is_none(), "no mirror DOM while disabled");
}

#[test]
fn enable_materializes_pending_tree() {
    let mut bridge = WebA11yBridge::new().expect("bridge");
    bridge.update(&build_update()).expect("update");
    bridge.set_enabled(true).expect("enable");
    assert_eq!(bridge.mirrored_count(), 6);
    let root = element(1).expect("root element");
    assert_eq!(root.get_attribute("role").as_deref(), Some("document"));
}

#[test]
fn mirror_elements_are_positioned_at_bounds() {
    let mut bridge = WebA11yBridge::new().expect("bridge");
    bridge.update(&build_update()).expect("update");
    bridge.set_enabled(true).expect("enable");
    let input = element(2).expect("input element");
    let style = input.get_attribute("style").unwrap_or_default();
    assert!(style.contains("position:absolute"), "{style}");
    assert!(style.contains("opacity:0"), "{style}");
    assert!(style.contains("width:200px"), "{style}");
    assert!(style.contains("height:30px"), "{style}");
}

#[test]
fn text_input_projects_real_input_element() {
    let mut bridge = WebA11yBridge::new().expect("bridge");
    bridge.update(&build_update()).expect("update");
    bridge.set_enabled(true).expect("enable");
    let input = element(2).expect("input element");
    assert_eq!(input.tag_name().to_ascii_lowercase(), "input");
    let typed = input
        .dyn_ref::<web_sys::HtmlInputElement>()
        .expect("HtmlInputElement");
    assert_eq!(typed.value(), "Ada");
}

#[test]
fn password_projects_type_password_without_value_leak() {
    let mut bridge = WebA11yBridge::new().expect("bridge");
    bridge.update(&build_update()).expect("update");
    bridge.set_enabled(true).expect("enable");
    let el = element(3).expect("password element");
    let typed = el
        .dyn_ref::<web_sys::HtmlInputElement>()
        .expect("HtmlInputElement");
    assert_eq!(typed.type_(), "password");
    // The secret lives only in the input's own value property.
    assert_eq!(typed.value(), "hunter2");
    assert!(el.get_attribute("aria-valuetext").is_none());
    assert!(
        el.outer_html().find("hunter2").is_none(),
        "password must not be serialized into markup"
    );
}

#[test]
fn managed_composite_removes_members_from_tab_order() {
    let mut bridge = WebA11yBridge::new().expect("bridge");
    bridge.update(&build_update()).expect("update");
    bridge.set_enabled(true).expect("enable");
    let listbox = element(4).expect("listbox");
    assert_eq!(
        listbox.get_attribute("aria-activedescendant").as_deref(),
        Some("martensite-a11y-6")
    );
    // Managed members leave the tab order entirely.
    assert_eq!(
        element(5).unwrap().get_attribute("tabindex").as_deref(),
        Some("-1")
    );
    assert_eq!(
        element(6).unwrap().get_attribute("tabindex").as_deref(),
        Some("-1")
    );
}
