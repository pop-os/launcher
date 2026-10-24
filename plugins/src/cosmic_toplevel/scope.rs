// SPDX-License-Identifier: GPL-3.0-only
// Copyright © 2026 System76

//! Decide which windows a window switcher lists for a [`WorkspaceScope`].
//!
//! Kept free of Wayland types so it can be tested directly. The rules mirror
//! the App List applet's `filter_top_levels` in cosmic-applets: a window whose
//! workspace is unknown, or listed while the active workspaces are not known
//! yet, counts as being on the active workspace rather than disappearing.

use pop_launcher::WorkspaceScope;

/// Where a window is, reduced to what the scope filter needs.
#[derive(Debug, Clone)]
pub struct WindowPlacement<W, O> {
    /// Workspaces the window is on; empty when the compositor didn't say.
    pub workspaces: Vec<W>,
    /// Outputs the window is shown on; empty when the compositor didn't say.
    pub outputs: Vec<O>,
    /// Whether this is the focused (activated) window.
    pub focused: bool,
}

/// Outputs of the focused window, or an empty list when nothing is focused.
pub fn focused_outputs<W, O: Clone>(windows: &[WindowPlacement<W, O>]) -> Vec<O> {
    windows
        .iter()
        .find(|window| window.focused)
        .map(|window| window.outputs.clone())
        .unwrap_or_default()
}

/// Whether `window` belongs in a window switcher with the given `scope`.
///
/// `active_workspaces` holds the workspace currently shown on each output.
/// `focused_outputs` is the result of [`focused_outputs`].
pub fn in_scope<W: PartialEq, O: PartialEq>(
    scope: WorkspaceScope,
    window: &WindowPlacement<W, O>,
    active_workspaces: &[W],
    focused_outputs: &[O],
) -> bool {
    let on_active_workspace = active_workspaces.is_empty()
        || window.workspaces.is_empty()
        || window
            .workspaces
            .iter()
            .any(|workspace| active_workspaces.contains(workspace));

    match scope {
        WorkspaceScope::AllWorkspaces => true,
        WorkspaceScope::ActiveWorkspaces => on_active_workspace,
        WorkspaceScope::FocusedOutputWorkspace => {
            // Without a focused window there is no "this display" to pick;
            // an empty list makes cosmic-launcher skip the switcher entirely.
            let on_focused_output = !focused_outputs.is_empty()
                && (window.outputs.is_empty()
                    || window
                        .outputs
                        .iter()
                        .any(|output| focused_outputs.contains(output)));

            on_active_workspace && on_focused_output
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pop_launcher::WorkspaceScope;

    // Two displays: workspace 1 is active on "A", workspace 5 on "B";
    // workspace 2 exists but isn't shown anywhere.
    const ACTIVE: &[u32] = &[1, 5];

    fn window(workspaces: &[u32], outputs: &[&'static str]) -> WindowPlacement<u32, &'static str> {
        WindowPlacement {
            workspaces: workspaces.to_vec(),
            outputs: outputs.to_vec(),
            focused: false,
        }
    }

    #[test]
    fn all_workspaces_keeps_everything() {
        assert!(in_scope(
            WorkspaceScope::AllWorkspaces,
            &window(&[2], &["A"]),
            ACTIVE,
            &[]
        ));
    }

    #[test]
    fn active_workspaces_keeps_windows_on_any_active_workspace() {
        let scope = WorkspaceScope::ActiveWorkspaces;
        assert!(in_scope(scope, &window(&[1], &["A"]), ACTIVE, &[]));
        assert!(in_scope(scope, &window(&[5], &["B"]), ACTIVE, &[]));
        assert!(!in_scope(scope, &window(&[2], &["A"]), ACTIVE, &[]));
    }

    #[test]
    fn unknown_workspace_info_keeps_window() {
        let scope = WorkspaceScope::ActiveWorkspaces;
        assert!(in_scope(scope, &window(&[2], &["A"]), &[], &[]));
        assert!(in_scope(scope, &window(&[], &["A"]), ACTIVE, &[]));
    }

    #[test]
    fn focused_output_keeps_only_active_workspace_on_focused_display() {
        let scope = WorkspaceScope::FocusedOutputWorkspace;
        assert!(in_scope(scope, &window(&[1], &["A"]), ACTIVE, &["A"]));
        assert!(!in_scope(scope, &window(&[5], &["B"]), ACTIVE, &["A"]));
        assert!(!in_scope(scope, &window(&[2], &["A"]), ACTIVE, &["A"]));
    }

    #[test]
    fn focused_output_without_focused_window_keeps_nothing() {
        let scope = WorkspaceScope::FocusedOutputWorkspace;
        assert!(!in_scope(scope, &window(&[1], &["A"]), ACTIVE, &[]));
    }

    #[test]
    fn window_without_output_info_is_kept_on_focused_output() {
        let scope = WorkspaceScope::FocusedOutputWorkspace;
        assert!(in_scope(scope, &window(&[1], &[]), ACTIVE, &["A"]));
    }

    #[test]
    fn focused_outputs_returns_outputs_of_activated_window() {
        let mut windows = vec![
            window(&[1], &["A"]),
            window(&[5], &["B"]),
            window(&[2], &["A"]),
        ];
        assert!(focused_outputs(&windows).is_empty());

        windows[1].focused = true;
        assert_eq!(focused_outputs(&windows), vec!["B"]);
    }
}
