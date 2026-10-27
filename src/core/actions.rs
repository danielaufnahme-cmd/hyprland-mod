//! Dispatcher actions and metadata for keybindings.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Action {
    pub id: String,
    pub label: String,
    pub description: String,
    pub has_arg: bool,
    pub arg_placeholder: Option<String>,
}

pub fn get_actions() -> Vec<Action> {
    vec![
        Action {
            id: "exec".into(),
            label: "Execute Application / Command".into(),
            description: "Launch an external application or run a shell command".into(),
            has_arg: true,
            arg_placeholder: Some("e.g. kitty, rofi -show drun".into()),
        },
        Action {
            id: "killactive".into(),
            label: "Close Active Window".into(),
            description: "Closes the focused window immediately".into(),
            has_arg: false,
            arg_placeholder: None,
        },
        Action {
            id: "togglefloating".into(),
            label: "Toggle Floating".into(),
            description: "Toggle active window between tiled and floating mode".into(),
            has_arg: false,
            arg_placeholder: None,
        },
        Action {
            id: "fullscreen".into(),
            label: "Toggle Fullscreen".into(),
            description: "Toggle active window fullscreen".into(),
            has_arg: false,
            arg_placeholder: None,
        },
        Action {
            id: "workspace".into(),
            label: "Switch Workspace".into(),
            description: "Switch active workspace to specified workspace".into(),
            has_arg: true,
            arg_placeholder: Some("workspace name or number (e.g. 1, 2, +1, e+1)".into()),
        },
        Action {
            id: "movetoworkspace".into(),
            label: "Move Window to Workspace".into(),
            description: "Move focused window to specified workspace".into(),
            has_arg: true,
            arg_placeholder: Some("workspace name or number (e.g. 1, 2)".into()),
        },
        Action {
            id: "movetoworkspacesilent".into(),
            label: "Move Window to Workspace (Silent)".into(),
            description: "Move focused window without switching focus to that workspace".into(),
            has_arg: true,
            arg_placeholder: Some("workspace name or number".into()),
        },
        Action {
            id: "togglesplit".into(),
            label: "Toggle Split (Dwindle)".into(),
            description: "Toggle horizontal/vertical split orientation".into(),
            has_arg: false,
            arg_placeholder: None,
        },
        Action {
            id: "pseudo".into(),
            label: "Toggle Pseudo-tile".into(),
            description: "Keep window size while tiling".into(),
            has_arg: false,
            arg_placeholder: None,
        },
        Action {
            id: "pin".into(),
            label: "Pin Window".into(),
            description: "Show active window across all workspaces".into(),
            has_arg: false,
            arg_placeholder: None,
        },
    ]
}
