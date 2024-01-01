pub use bevy::prelude::*;

use crate::EditorResource;

pub struct EditorInputPlugin;
impl Plugin for EditorInputPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, editor_input_system);
    }
}

pub fn editor_input_system(mut editor: ResMut<EditorResource>, kb: Res<Input<KeyCode>>) {
    if kb.just_pressed(KeyCode::F1) {
        editor.0 = !editor.0;
    }
}
