use super::super::*;
use super::helpers::test_invocation;

const STATE: &str = "Lwinedroid/tests/RotaryState;";

fn generic_motion_contract_program() -> DexProgram {
    let dex_file = crate::dex::parse_dex_file(
        "generic_motion_contract.dex",
        include_bytes!("fixtures/generic_motion_contract.dex").to_vec(),
    )
    .expect("generic motion contract DEX should parse");
    DexProgram::new(vec![dex_file])
}

fn state_int(program: &DexProgram, name: &str) -> i32 {
    let field = DexFieldId {
        class_descriptor: STATE.to_owned(),
        type_descriptor: "I".to_owned(),
        name: name.to_owned(),
    };
    match program.read_stored_static_field(&field) {
        Some(DexValue::Int(value)) => value,
        None => 0,
        Some(other) => panic!("{name} should be an int, got {other}"),
    }
}

fn state_float(program: &DexProgram, name: &str) -> f32 {
    let field = DexFieldId {
        class_descriptor: STATE.to_owned(),
        type_descriptor: "F".to_owned(),
        name: name.to_owned(),
    };
    match program.read_stored_static_field(&field) {
        Some(DexValue::Float(value)) => value,
        None => 0.0,
        Some(other) => panic!("{name} should be a float, got {other}"),
    }
}

fn attach(program: &DexProgram, parent: &DexValue, child: &DexValue) {
    let mut runtime = program.runtime.borrow_mut();
    runtime
        .view_parents
        .insert(map_identity_key(child), parent.clone());
    runtime
        .view_group_children
        .entry(map_identity_key(parent))
        .or_default()
        .push(child.clone());
}

// cite: frameworks-base ViewRootImpl.java#processGenericMotionEvent
// cite: frameworks-base ViewGroup.java#dispatchGenericFocusedEvent, mFocused
#[test]
fn rotary_without_focused_view_is_not_delivered() {
    let program = generic_motion_contract_program();
    let root = program.allocate_object("Landroid/widget/FrameLayout;".to_owned());
    let view = program.allocate_object("Lwinedroid/tests/RotaryOverrideView;".to_owned());
    attach(&program, &root, &view);

    let handled = program
        .perform_runtime_view_rotary_scroll(-1.0, 0)
        .expect("rotary dispatch should run");

    assert_eq!(handled, 0);
    assert_eq!(state_int(&program, "overrideHits"), 0);
}

// cite: frameworks-base View.java#dispatchGenericMotionEvent, dispatchGenericMotionEventInternal
// cite: frameworks-base MotionEvent.java#ACTION_SCROLL, AXIS_SCROLL, getAxisValue
// cite: frameworks-base InputDevice.java#SOURCE_ROTARY_ENCODER
#[test]
fn rotary_reaches_focused_view_on_generic_motion_event_with_axis_scroll() {
    let program = generic_motion_contract_program();
    let root = program.allocate_object("Landroid/widget/FrameLayout;".to_owned());
    let sibling = program.allocate_object("Lwinedroid/tests/RotaryOverrideView;".to_owned());
    let focused = program.allocate_object("Lwinedroid/tests/RotaryOverrideView;".to_owned());
    attach(&program, &root, &sibling);
    attach(&program, &root, &focused);
    program.set_focused_view(Some(focused));

    let handled = program
        .perform_runtime_view_rotary_scroll(-2.5, 0)
        .expect("rotary dispatch should run");

    assert_eq!(handled, 1);
    assert_eq!(state_int(&program, "overrideHits"), 1);
    assert_eq!(state_float(&program, "overrideAxis"), -2.5);
}

// cite: frameworks-base View.java#dispatchGenericMotionEvent
#[test]
fn ancestor_dispatch_override_consumes_before_focused_child() {
    let program = generic_motion_contract_program();
    let group = program.allocate_object("Lwinedroid/tests/RotaryConsumingGroup;".to_owned());
    let focused = program.allocate_object("Lwinedroid/tests/RotaryOverrideView;".to_owned());
    attach(&program, &group, &focused);
    program.set_focused_view(Some(focused));

    let handled = program
        .perform_runtime_view_rotary_scroll(1.0, 0)
        .expect("rotary dispatch should run");

    assert_eq!(handled, 1);
    assert_eq!(state_int(&program, "consumingHits"), 1);
    assert_eq!(state_int(&program, "overrideHits"), 0);
}

// cite: frameworks-base ViewGroup.java#dispatchGenericFocusedEvent
// cite: frameworks-base View.java#dispatchGenericMotionEvent
#[test]
fn ancestor_super_dispatch_continues_down_the_focus_chain() {
    let program = generic_motion_contract_program();
    let group = program.allocate_object("Lwinedroid/tests/RotaryInterceptGroup;".to_owned());
    let focused = program.allocate_object("Lwinedroid/tests/RotaryOverrideView;".to_owned());
    attach(&program, &group, &focused);
    program.set_focused_view(Some(focused));

    let handled = program
        .perform_runtime_view_rotary_scroll(1.0, 0)
        .expect("rotary dispatch should run");

    assert_eq!(handled, 1);
    assert_eq!(state_int(&program, "interceptHits"), 1);
    assert_eq!(state_int(&program, "overrideHits"), 1);
    assert_eq!(state_int(&program, "groupFallbackHits"), 0);
}

// cite: frameworks-base View.java#dispatchGenericMotionEvent, dispatchGenericMotionEventInternal, onGenericMotionEvent
#[test]
fn unhandled_focused_view_unwinds_to_ancestor_on_generic_motion_event() {
    let program = generic_motion_contract_program();
    let group = program.allocate_object("Lwinedroid/tests/RotaryInterceptGroup;".to_owned());
    let focused = program.allocate_object("Landroid/view/View;".to_owned());
    attach(&program, &group, &focused);
    program.set_focused_view(Some(focused));

    let handled = program
        .perform_runtime_view_rotary_scroll(1.0, 0)
        .expect("rotary dispatch should run");

    assert_eq!(handled, 1);
    assert_eq!(state_int(&program, "groupFallbackHits"), 1);
}

// cite: frameworks-base View.java#dispatchGenericMotionEventInternal, setOnGenericMotionListener
#[test]
fn generic_motion_listener_runs_before_on_generic_motion_event() {
    let program = generic_motion_contract_program();
    let root = program.allocate_object("Landroid/widget/FrameLayout;".to_owned());
    let focused = program.allocate_object("Lwinedroid/tests/RotaryOverrideView;".to_owned());
    let listener = program.allocate_object("Lwinedroid/tests/RotaryListener;".to_owned());
    attach(&program, &root, &focused);
    let set_listener = DexMethodId {
        class_descriptor: "Landroid/view/View;".to_owned(),
        name: "setOnGenericMotionListener".to_owned(),
        proto: crate::dex::DexProto {
            shorty: "VL".to_owned(),
            return_type: "V".to_owned(),
            parameters: vec!["Landroid/view/View$OnGenericMotionListener;".to_owned()],
        },
    };
    assert_eq!(
        program.invoke_builtin_method(
            &set_listener,
            Some(&test_invocation(vec![focused.clone(), listener]))
        ),
        Some(DexExecutionResult::Void)
    );
    program.set_focused_view(Some(focused));

    let handled = program
        .perform_runtime_view_rotary_scroll(1.0, 0)
        .expect("rotary dispatch should run");

    assert_eq!(handled, 1);
    assert_eq!(state_int(&program, "listenerHits"), 1);
    assert_eq!(state_int(&program, "overrideHits"), 0);
}
