//! Authoritative editable routing graph (egui canvas).

use crate::app::CottApp;
use cott_core::graph::{GraphError, PortType};
use cott_core::ids::{EdgeId, NodeId, PortId};
use cott_ipc::PluginDescriptor;
use eframe::egui;
use indexmap::IndexMap;

const MIN_ZOOM: f32 = 0.25;
const MAX_ZOOM: f32 = 4.0;
const MIN_ZOOM_PCT: i32 = 25;
const MAX_ZOOM_PCT: i32 = 400;
/// Scroll-wheel zoom step (percent). Always lands on a multiple of this step.
const SCROLL_ZOOM_STEP_PCT: i32 = 6;
/// Toolbar +/- zoom step (percent). Always lands on a multiple of 5.
const BUTTON_ZOOM_STEP_PCT: i32 = 5;

enum AddNodeAction {
    Gain,
    Mixer,
    Instrument(PluginDescriptor),
    Effect(PluginDescriptor),
}

enum ContextAction {
    Add(AddNodeAction),
    DeleteNode(NodeId),
    DeleteEdge(EdgeId),
    OpenEditor(NodeId),
}

pub fn draw(app: &mut CottApp, ui: &mut egui::Ui) {
    let mut pending_zoom_pct: Option<i32> = None;

    ui.horizontal(|ui| {
        ui.label(
            "Drag nodes · empty drag pans · scroll zooms · double-click plugin for editor · right-click add/delete",
        );
        if ui.button("Add Gain").clicked() {
            let mut node = cott_core::graph::GraphNode::stereo_gain_pan("Gain");
            node.position = [200.0, 200.0];
            let id = node.id;
            app.commands.push(
                &mut app.project,
                cott_core::commands::Command::AddNode { node },
            );
            app.ui.selected_node = Some(id);
            app.sync_engine();
        }
        if ui.button("Add Mixer").clicked() {
            let mut node = cott_core::graph::GraphNode::sum_mixer("Bus");
            node.position = [300.0, 200.0];
            let id = node.id;
            app.commands.push(
                &mut app.project,
                cott_core::commands::Command::AddNode { node },
            );
            app.ui.selected_node = Some(id);
            app.sync_engine();
        }
        ui.separator();
        if ui.button("−").on_hover_text("Zoom out (−5%)").clicked() {
            pending_zoom_pct = Some(step_zoom_percent(
                zoom_to_percent(app.ui.graph_zoom),
                -BUTTON_ZOOM_STEP_PCT,
                BUTTON_ZOOM_STEP_PCT,
            ));
        }
        let mut zoom_pct = zoom_to_percent(app.ui.graph_zoom);
        let zoom_edit = ui.add(
            egui::DragValue::new(&mut zoom_pct)
                .range(MIN_ZOOM_PCT as f64..=MAX_ZOOM_PCT as f64)
                .suffix("%")
                .speed(1.0)
                .clamp_existing_to_range(true),
        );
        if zoom_edit.changed() {
            pending_zoom_pct = Some(zoom_pct.clamp(MIN_ZOOM_PCT, MAX_ZOOM_PCT));
        }
        if ui.button("+").on_hover_text("Zoom in (+5%)").clicked() {
            pending_zoom_pct = Some(step_zoom_percent(
                zoom_to_percent(app.ui.graph_zoom),
                BUTTON_ZOOM_STEP_PCT,
                BUTTON_ZOOM_STEP_PCT,
            ));
        }
        if ui.button("Reset view").clicked() {
            app.ui.graph_pan = egui::Vec2::ZERO;
            app.ui.graph_zoom = 1.0;
        }
    });

    // Drag identity is stored on CottApp so it survives frame-to-frame id churn.
    let (rect, resp) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());

    // Keep world content visually stable when the lower panel (or canvas) moves.
    if let Some(prev) = app.ui.graph_canvas_origin {
        let origin_delta = rect.min - prev;
        if origin_delta != egui::Vec2::ZERO {
            app.ui.graph_pan -= origin_delta;
        }
    }
    app.ui.graph_canvas_origin = Some(rect.min);

    let pointer = resp
        .interact_pointer_pos()
        .or_else(|| ui.ctx().pointer_interact_pos())
        .or_else(|| ui.ctx().pointer_latest_pos());

    // Toolbar zoom (exact entry or +/-) around the canvas center.
    if let Some(pct) = pending_zoom_pct {
        set_zoom_percent_at(app, rect.center(), pct);
    }

    // Scroll / pinch zoom in steps of 2%, anchored under the cursor.
    // Use raw_scroll_delta so one physical wheel notch = one 2% step
    // (smooth_scroll_delta stays non-zero across many frames and overshoots).
    if resp.hovered() {
        let step_dir = ui.input(|i| {
            let raw = i.raw_scroll_delta.y;
            if raw.abs() > f32::EPSILON {
                if raw > 0.0 {
                    1
                } else {
                    -1
                }
            } else {
                // Pinch-to-zoom only (ignore tiny scroll-derived zoom deltas).
                let pinch = i.zoom_delta();
                if pinch > 1.05 {
                    1
                } else if pinch < 0.95 {
                    -1
                } else {
                    0
                }
            }
        });
        if step_dir != 0 {
            let anchor = pointer.unwrap_or_else(|| rect.center());
            let pct = step_zoom_percent(
                zoom_to_percent(app.ui.graph_zoom),
                step_dir * SCROLL_ZOOM_STEP_PCT,
                SCROLL_ZOOM_STEP_PCT,
            );
            set_zoom_percent_at(app, anchor, pct);
        }
    }

    let pan = app.ui.graph_pan;
    let zoom = app.ui.graph_zoom.clamp(MIN_ZOOM, MAX_ZOOM);
    let world_to_screen = |wx: f32, wy: f32| -> egui::Pos2 {
        egui::pos2(
            rect.left() + pan.x + wx * zoom,
            rect.top() + pan.y + wy * zoom,
        )
    };
    let screen_to_world = |pos: egui::Pos2| -> [f32; 2] {
        [
            (pos.x - rect.left() - pan.x) / zoom,
            (pos.y - rect.top() - pan.y) / zoom,
        ]
    };

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(24, 26, 30));

    let node_w = 140.0;
    let node_h_base = 56.0;
    let mut port_positions: IndexMap<(NodeId, PortId), egui::Pos2> = IndexMap::new();
    let mut node_rects: IndexMap<NodeId, egui::Rect> = IndexMap::new();

    // Draw edges.
    let mut edge_hit: Option<EdgeId> = None;
    for edge in app.project.graph.edges.values() {
        let Some(from_node) = app.project.graph.nodes.get(&edge.from_node) else {
            continue;
        };
        let Some(to_node) = app.project.graph.nodes.get(&edge.to_node) else {
            continue;
        };
        let from_idx = from_node
            .outputs
            .iter()
            .position(|p| p.id == edge.from_port)
            .unwrap_or(0);
        let to_idx = to_node
            .inputs
            .iter()
            .position(|p| p.id == edge.to_port)
            .unwrap_or(0);
        let a = world_to_screen(
            from_node.position[0] + node_w,
            from_node.position[1] + 28.0 + from_idx as f32 * 14.0,
        );
        let b = world_to_screen(
            to_node.position[0],
            to_node.position[1] + 28.0 + to_idx as f32 * 14.0,
        );
        let color = match from_node
            .outputs
            .iter()
            .find(|p| p.id == edge.from_port)
            .map(|p| p.port_type)
        {
            Some(PortType::Midi) => egui::Color32::from_rgb(220, 180, 80),
            _ => egui::Color32::from_rgb(80, 180, 220),
        };
        painter.line_segment([a, b], egui::Stroke::new(2.0, color));
        if let Some(pos) = pointer {
            if distance_to_segment(pos, a, b) < 8.0 {
                edge_hit = Some(edge.id);
            }
        }
    }
    if let Some(edge_id) = edge_hit {
        if let Some(edge) = app.project.graph.edges.get(&edge_id) {
            if let (Some(from_node), Some(to_node)) = (
                app.project.graph.nodes.get(&edge.from_node),
                app.project.graph.nodes.get(&edge.to_node),
            ) {
                let from_idx = from_node
                    .outputs
                    .iter()
                    .position(|p| p.id == edge.from_port)
                    .unwrap_or(0);
                let to_idx = to_node
                    .inputs
                    .iter()
                    .position(|p| p.id == edge.to_port)
                    .unwrap_or(0);
                let a = world_to_screen(
                    from_node.position[0] + node_w,
                    from_node.position[1] + 28.0 + from_idx as f32 * 14.0,
                );
                let b = world_to_screen(
                    to_node.position[0],
                    to_node.position[1] + 28.0 + to_idx as f32 * 14.0,
                );
                painter.line_segment([a, b], egui::Stroke::new(3.0, egui::Color32::WHITE));
            }
        }
    }

    // Draw nodes.
    let node_ids: Vec<_> = app.project.graph.nodes.keys().copied().collect();
    for node_id in node_ids {
        let Some(node) = app.project.graph.nodes.get(&node_id).cloned() else {
            continue;
        };
        let ports = node.inputs.len().max(node.outputs.len()).max(1);
        let h = node_h_base + (ports as f32 - 1.0) * 14.0;
        let origin = world_to_screen(node.position[0], node.position[1]);
        let nrect = egui::Rect::from_min_size(origin, egui::vec2(node_w * zoom, h * zoom));
        node_rects.insert(node_id, nrect);
        let selected = app.ui.selected_node == Some(node_id);
        painter.rect_filled(
            nrect,
            6.0 * zoom,
            if selected {
                egui::Color32::from_rgb(60, 80, 110)
            } else {
                egui::Color32::from_rgb(48, 52, 60)
            },
        );
        painter.rect_stroke(
            nrect,
            6.0 * zoom,
            egui::Stroke::new(1.0, egui::Color32::WHITE),
            egui::StrokeKind::Outside,
        );
        painter.text(
            nrect.left_top() + egui::vec2(8.0 * zoom, 6.0 * zoom),
            egui::Align2::LEFT_TOP,
            &node.name,
            egui::FontId::proportional((13.0 * zoom).clamp(9.0, 28.0)),
            egui::Color32::WHITE,
        );

        let port_r = (5.0 * zoom).clamp(3.0, 12.0);
        for (i, port) in node.inputs.iter().enumerate() {
            let p = world_to_screen(
                node.position[0],
                node.position[1] + 28.0 + i as f32 * 14.0,
            );
            port_positions.insert((node_id, port.id), p);
            let color = match port.port_type {
                PortType::Midi => egui::Color32::from_rgb(220, 180, 80),
                PortType::Audio => egui::Color32::from_rgb(80, 180, 220),
            };
            painter.circle_filled(p, port_r, color);
        }
        for (i, port) in node.outputs.iter().enumerate() {
            let p = world_to_screen(
                node.position[0] + node_w,
                node.position[1] + 28.0 + i as f32 * 14.0,
            );
            port_positions.insert((node_id, port.id), p);
            let color = match port.port_type {
                PortType::Midi => egui::Color32::from_rgb(220, 180, 80),
                PortType::Audio => egui::Color32::from_rgb(80, 180, 220),
            };
            painter.circle_filled(p, port_r, color);
        }
    }

    // Only start canvas interactions when the press is inside the canvas
    // (ignore lower-panel resize grip, tabs, toolbar).
    let primary_down = ui.input(|i| i.pointer.primary_down());
    let primary_pressed = ui.input(|i| i.pointer.primary_pressed());
    let primary_released = ui.input(|i| i.pointer.primary_released());
    let pointer_in_canvas = pointer.is_some_and(|pos| rect.contains(pos));

    if primary_pressed && pointer_in_canvas {
        if let Some(pos) = pointer {
            let mut started_connect = false;
            for ((node_id, port_id), port_pos) in &port_positions {
                let Some(node) = app.project.graph.nodes.get(node_id) else {
                    continue;
                };
                if node.outputs.iter().any(|p| p.id == *port_id) && port_pos.distance(pos) < 12.0 {
                    app.ui.graph_connect_from = Some((*node_id, *port_id));
                    app.ui.graph_drag_node = None;
                    app.ui.graph_panning = false;
                    started_connect = true;
                    break;
                }
            }
            if !started_connect {
                app.ui.graph_connect_from = None;
                let mut hit = None;
                for (node_id, nrect) in node_rects.iter().rev() {
                    if nrect.contains(pos) {
                        hit = Some(*node_id);
                        break;
                    }
                }
                if let Some(id) = hit {
                    app.ui.graph_drag_node = Some(id);
                    app.ui.selected_node = Some(id);
                    app.ui.graph_panning = false;
                } else {
                    // Empty space → pan the canvas.
                    app.ui.graph_drag_node = None;
                    app.ui.graph_panning = true;
                }
            }
        }
    }

    if primary_down
        && (app.ui.graph_drag_node.is_some()
            || app.ui.graph_connect_from.is_some()
            || app.ui.graph_panning)
    {
        let delta = ui.ctx().input(|i| i.pointer.delta());
        if app.ui.graph_panning {
            app.ui.graph_pan += delta;
        } else if let Some(id) = app.ui.graph_drag_node {
            if let Some(node) = app.project.graph.nodes.get_mut(&id) {
                node.position[0] += delta.x / zoom;
                node.position[1] += delta.y / zoom;
            }
        }
        if let Some((from_node, from_port)) = app.ui.graph_connect_from {
            if let Some(pos) = pointer {
                if let Some(start) = port_positions.get(&(from_node, from_port)) {
                    painter
                        .line_segment([*start, pos], egui::Stroke::new(2.0, egui::Color32::YELLOW));
                }
            }
        }
    }

    if primary_released {
        if let Some((from_node, from_port)) = app.ui.graph_connect_from.take() {
            if let Some(pos) = pointer {
                if let Some((to_node, to_port)) =
                    hit_input_port(&port_positions, &app.project.graph, pos)
                {
                    let replaced: Vec<_> = app
                        .project
                        .graph
                        .edges
                        .values()
                        .filter(|e| e.to_node == to_node && e.to_port == to_port)
                        .cloned()
                        .collect();
                    match app
                        .project
                        .graph
                        .connect_replace(from_node, from_port, to_node, to_port)
                    {
                        Ok(edge_id) => {
                            let edge = app.project.graph.edges[&edge_id].clone();
                            app.commands.record(cott_core::commands::Command::ConnectReplace {
                                edge,
                                replaced,
                            });
                            app.status = "Connected".into();
                            app.sync_engine();
                        }
                        Err(GraphError::Cycle) => {
                            app.status = "Rejected: would create a feedback loop".into();
                        }
                        Err(e) => app.status = format!("Connect failed: {e}"),
                    }
                }
            }
        }
        app.ui.graph_drag_node = None;
        app.ui.graph_panning = false;
    }

    // Double-click a plugin node to open its native editor.
    if resp.double_clicked() {
        if let Some(pos) = pointer {
            for (node_id, nrect) in node_rects.iter().rev() {
                if nrect.contains(pos) {
                    app.ui.selected_node = Some(*node_id);
                    app.open_plugin_editor_for_node(*node_id);
                    break;
                }
            }
        }
    }

    let canvas_id = egui::Id::new("cott_routing_canvas");
    let mut context_node: Option<NodeId> = None;
    if resp.secondary_clicked() {
        app.ui.graph_connect_from = None;
        app.ui.graph_drag_node = None;
        app.ui.graph_panning = false;
        if let Some(pos) = pointer {
            let graph_position = screen_to_world(pos);
            ui.ctx()
                .data_mut(|data| data.insert_temp(canvas_id.with("context_pos"), graph_position));
            for (node_id, nrect) in node_rects.iter().rev() {
                if nrect.contains(pos) {
                    context_node = Some(*node_id);
                    app.ui.selected_node = Some(*node_id);
                    break;
                }
            }
            ui.ctx()
                .data_mut(|data| data.insert_temp(canvas_id.with("context_node"), context_node));
            ui.ctx()
                .data_mut(|data| data.insert_temp(canvas_id.with("context_edge"), edge_hit));
        }
    }

    let graph_position = ui.ctx().data(|data| {
        data.get_temp::<[f32; 2]>(canvas_id.with("context_pos"))
            .unwrap_or([200.0, 200.0])
    });
    let menu_node: Option<NodeId> = ui
        .ctx()
        .data(|data| data.get_temp(canvas_id.with("context_node")))
        .flatten();
    let menu_edge: Option<EdgeId> = ui
        .ctx()
        .data(|data| data.get_temp(canvas_id.with("context_edge")))
        .flatten();
    let plugins = app.plugin_host.lock().catalog.clone();
    let mut action = None;

    resp.context_menu(|ui| {
        if let Some(edge_id) = menu_edge {
            if ui.button("Delete connection").clicked() {
                action = Some(ContextAction::DeleteEdge(edge_id));
                ui.close_menu();
            }
            ui.separator();
        }
        if let Some(node_id) = menu_node {
            if node_has_plugin_editor(&app.project.graph, node_id) {
                if ui.button("Open plugin editor").clicked() {
                    action = Some(ContextAction::OpenEditor(node_id));
                    ui.close_menu();
                }
            }
            if app.can_remove_graph_node(node_id) {
                let delete_label = match app.project.graph.nodes.get(&node_id).map(|n| &n.kind) {
                    Some(
                        cott_core::graph::NodeKind::Vst3Effect { .. }
                        | cott_core::graph::NodeKind::Vst3Instrument { .. },
                    ) => "Delete plugin / FX",
                    _ => "Delete node",
                };
                if ui.button(delete_label).clicked() {
                    action = Some(ContextAction::DeleteNode(node_id));
                    ui.close_menu();
                }
                ui.separator();
            } else {
                ui.weak("This node is required by the track");
                ui.separator();
            }
        }
        ui.label("Add to routing");
        ui.separator();
        if ui.button("Gain").clicked() {
            action = Some(ContextAction::Add(AddNodeAction::Gain));
            ui.close_menu();
        }
        if ui.button("Mixer").clicked() {
            action = Some(ContextAction::Add(AddNodeAction::Mixer));
            ui.close_menu();
        }
        ui.separator();
        ui.menu_button("Instrument (MIDI)", |ui| {
            let mut found = false;
            for plugin in plugins.iter().filter(|plugin| plugin.is_instrument) {
                found = true;
                if ui.button(&plugin.name).clicked() {
                    action = Some(ContextAction::Add(AddNodeAction::Instrument(
                        plugin.clone(),
                    )));
                    ui.close_menu();
                }
            }
            if !found {
                ui.weak("No instruments found");
            }
        });
        ui.menu_button("Effect (audio)", |ui| {
            let mut found = false;
            for plugin in plugins.iter().filter(|plugin| plugin.is_effect) {
                found = true;
                if ui.button(&plugin.name).clicked() {
                    action = Some(ContextAction::Add(AddNodeAction::Effect(plugin.clone())));
                    ui.close_menu();
                }
            }
            if !found {
                ui.weak("No effects found");
            }
        });
    });

    match action {
        Some(ContextAction::Add(AddNodeAction::Gain)) => {
            let mut node = cott_core::graph::GraphNode::stereo_gain_pan("Gain");
            node.position = graph_position;
            let id = node.id;
            app.commands.push(
                &mut app.project,
                cott_core::commands::Command::AddNode { node },
            );
            app.ui.selected_node = Some(id);
            app.sync_engine();
        }
        Some(ContextAction::Add(AddNodeAction::Mixer)) => {
            let mut node = cott_core::graph::GraphNode::sum_mixer("Bus");
            node.position = graph_position;
            let id = node.id;
            app.commands.push(
                &mut app.project,
                cott_core::commands::Command::AddNode { node },
            );
            app.ui.selected_node = Some(id);
            app.sync_engine();
        }
        Some(ContextAction::Add(AddNodeAction::Instrument(plugin))) => {
            app.load_instrument_on_selected_track(
                plugin.uid,
                plugin.path,
                plugin.name,
                graph_position,
            );
        }
        Some(ContextAction::Add(AddNodeAction::Effect(plugin))) => {
            app.load_effect(plugin.uid, plugin.path, plugin.name, graph_position);
        }
        Some(ContextAction::DeleteNode(node_id)) => {
            app.remove_graph_node(node_id);
        }
        Some(ContextAction::DeleteEdge(edge_id)) => {
            if let Some(edge) = app.project.graph.disconnect(edge_id) {
                app.commands
                    .record(cott_core::commands::Command::Disconnect { edge });
                app.status = "Connection removed".into();
                app.sync_engine();
            }
        }
        Some(ContextAction::OpenEditor(node_id)) => {
            app.open_plugin_editor_for_node(node_id);
        }
        None => {}
    }
}

fn zoom_to_percent(zoom: f32) -> i32 {
    (zoom.clamp(MIN_ZOOM, MAX_ZOOM) * 100.0).round() as i32
}

fn percent_to_zoom(pct: i32) -> f32 {
    (pct.clamp(MIN_ZOOM_PCT, MAX_ZOOM_PCT) as f32 / 100.0).clamp(MIN_ZOOM, MAX_ZOOM)
}

/// Step zoom by `delta_pct`, snapping the result onto a multiple of `grid`.
fn step_zoom_percent(current_pct: i32, delta_pct: i32, grid: i32) -> i32 {
    debug_assert!(grid > 0);
    let pct = current_pct.clamp(MIN_ZOOM_PCT, MAX_ZOOM_PCT);
    let ceil_to_grid = |v: i32| ((v + grid - 1) / grid) * grid;
    let floor_to_grid = |v: i32| (v / grid) * grid;
    let next = if delta_pct > 0 {
        if pct % grid == 0 {
            pct + delta_pct
        } else {
            ceil_to_grid(pct)
        }
    } else if delta_pct < 0 {
        if pct % grid == 0 {
            pct + delta_pct
        } else {
            floor_to_grid(pct)
        }
    } else {
        pct
    };
    let snapped = if next % grid == 0 {
        next
    } else if delta_pct >= 0 {
        ceil_to_grid(next)
    } else {
        floor_to_grid(next)
    };
    snapped.clamp(MIN_ZOOM_PCT, MAX_ZOOM_PCT)
}

fn set_zoom_percent_at(app: &mut CottApp, screen_pos: egui::Pos2, percent: i32) {
    let old_zoom = app.ui.graph_zoom.clamp(MIN_ZOOM, MAX_ZOOM);
    let new_zoom = percent_to_zoom(percent);
    if (new_zoom - old_zoom).abs() < f32::EPSILON {
        return;
    }
    let origin = app.ui.graph_canvas_origin.unwrap_or(egui::Pos2::ZERO);
    let world = [
        (screen_pos.x - origin.x - app.ui.graph_pan.x) / old_zoom,
        (screen_pos.y - origin.y - app.ui.graph_pan.y) / old_zoom,
    ];
    app.ui.graph_pan = egui::vec2(
        screen_pos.x - origin.x - world[0] * new_zoom,
        screen_pos.y - origin.y - world[1] * new_zoom,
    );
    app.ui.graph_zoom = new_zoom;
}

fn node_has_plugin_editor(graph: &cott_core::graph::AudioGraph, node_id: NodeId) -> bool {
    matches!(
        graph.nodes.get(&node_id).map(|n| &n.kind),
        Some(
            cott_core::graph::NodeKind::Vst3Instrument { failed, .. }
                | cott_core::graph::NodeKind::Vst3Effect { failed, .. }
        ) if !*failed
    )
}

fn hit_input_port(
    port_positions: &IndexMap<(NodeId, PortId), egui::Pos2>,
    graph: &cott_core::graph::AudioGraph,
    pos: egui::Pos2,
) -> Option<(NodeId, PortId)> {
    let mut best = None;
    let mut best_dist = 14.0_f32;
    for ((node_id, port_id), port_pos) in port_positions {
        let Some(node) = graph.nodes.get(node_id) else {
            continue;
        };
        if !node.inputs.iter().any(|p| p.id == *port_id) {
            continue;
        }
        let dist = port_pos.distance(pos);
        if dist < best_dist {
            best_dist = dist;
            best = Some((*node_id, *port_id));
        }
    }
    best
}

fn distance_to_segment(p: egui::Pos2, a: egui::Pos2, b: egui::Pos2) -> f32 {
    let ab = b - a;
    let len_sq = ab.length_sq();
    if len_sq <= f32::EPSILON {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    let proj = a + ab * t;
    p.distance(proj)
}
