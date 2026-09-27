fn push_history(stack: &mut Vec<EditSnapshot>, snapshot: EditSnapshot) {
    stack.push(snapshot);
    let mut bytes = stack.iter().map(|entry| entry.approx_bytes).sum::<usize>();
    while stack.len() > MAX_HISTORY_OPERATIONS || bytes > MAX_HISTORY_BYTES {
        let removed = stack.remove(0);
        bytes = bytes.saturating_sub(removed.approx_bytes);
    }
}

fn keyboard_shortcut(chord: &labello_domain::KeyChord) -> Option<egui::KeyboardShortcut> {
    let key = parse_key(&chord.key)?;
    let mut modifiers = egui::Modifiers::NONE;
    modifiers.command = chord.ctrl || chord.command;
    modifiers.shift = chord.shift;
    modifiers.alt = chord.alt;
    Some(egui::KeyboardShortcut::new(modifiers, key))
}

fn consume_keyboard_shortcut(ctx: &egui::Context, chord: &labello_domain::KeyChord) -> bool {
    let Some(shortcut) = keyboard_shortcut(chord) else {
        return false;
    };
    if ctx.input_mut(|input| input.consume_shortcut(&shortcut)) {
        return true;
    }
    if chord.ctrl || chord.command {
        let mut ctrl_shortcut = shortcut;
        ctrl_shortcut.modifiers.command = false;
        ctrl_shortcut.modifiers.ctrl = true;
        return ctx.input_mut(|input| input.consume_shortcut(&ctrl_shortcut));
    }
    false
}

fn consume_completion_shortcut(ctx: &egui::Context, chord: &labello_domain::KeyChord) -> bool {
    let Some(shortcut) = keyboard_shortcut(chord) else {
        return false;
    };
    let mut ctrl_modifiers = shortcut.modifiers;
    ctrl_modifiers.command = false;
    ctrl_modifiers.ctrl = true;
    ctx.input_mut(|input| {
        let mut fresh_press = false;
        input.events.retain(|event| {
            if let egui::Event::Key { key, modifiers, pressed: true, repeat, .. } = event
                && *key == shortcut.logical_key
                && (modifiers.matches_logically(shortcut.modifiers)
                    || (chord.ctrl || chord.command) && modifiers.matches_logically(ctrl_modifiers))
            {
                fresh_press |= !repeat;
                // Consume repeats too: they must not fall through to a focused button.
                false
            } else {
                true
            }
        });
        fresh_press
    })
}

pub(crate) fn parse_key(key: &str) -> Option<egui::Key> {
    egui::Key::from_name(key)
}

pub(crate) fn tool_for_annotation_type(annotation_type: &AnnotationType) -> Tool {
    match annotation_type {
        AnnotationType::BoundingBox => Tool::BoundingBox,
        AnnotationType::Skeleton => Tool::Keypoints,
    }
}

fn valid_workflow(task: &TaskDefinition) -> bool {
    task.enabled && task.class_ids.len() == 1
}

pub(crate) fn annotation_type_label(annotation_type: &AnnotationType) -> &'static str {
    match annotation_type {
        AnnotationType::BoundingBox => "bounding box",
        AnnotationType::Skeleton => "skeleton",
    }
}
