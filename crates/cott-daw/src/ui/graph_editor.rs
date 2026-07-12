//! Authoritative editable routing graph (egui canvas).

use crate::app::CottApp;
use cott_core::graph::{GraphError, PortType};
use cott_core::ids::{EdgeId, NodeId, PortId};
use cott_ipc::PluginDescriptor;
use eframe::egui;
use indexmap::IndexMap;

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
    ui.horizontal(|ui| {
        ui.label(
            "Drag nodes · empty drag pans · double-click plugin for editor · right-click add/delete",
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
        if ui.button("Reset view").clicked() {
            app.ui.graph_pan = egui::Vec2::ZERO;
        }
    });

    // Drag identity is stored on CottApp so it survives frame-to-frame id churn.
    let (rect, resp) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(24, 26, 30));

    let pan = app.ui.graph_pan;
    let node_w = 140.0;
    let node_h_base = 56.0;
    let mut port_positions: IndexMap<(NodeId, PortId), egui::Pos2> = IndexMap::new();
    let mut node_rects: IndexMap<NodeId, egui::Rect> = IndexMap::new();

    let pointer = resp
        .interact_pointer_pos()
        .or_else(|| ui.ctx().pointer_interact_pos())
        .or_else(|| ui.ctx().pointer_latest_pos());

    let world_to_screen = |wx: f32, wy: f32| -> egui::Pos2 {
        egui::pos2(rect.left() + wx + pan.x, rect.top() + wy + pan.y)
    };

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
        let nrect = egui::Rect::from_min_size(origin, egui::vec2(node_w, h));
        node_rects.insert(node_id, nrect);
        let selected = app.ui.selected_node == Some(node_id);
        painter.rect_filled(
            nrect,
            6.0,
            if selected {
                egui::Color32::from_rgb(60, 80, 110)
            } else {
                egui::Color32::from_rgb(48, 52, 60)
            },
        );
        painter.rect_stroke(
            nrect,
            6.0,
            egui::Stroke::new(1.0, egui::Color32::WHITE),
            egui::StrokeKind::Outside,
        );
        painter.text(
            nrect.left_top() + egui::vec2(8.0, 6.0),
            egui::Align2::LEFT_TOP,
            &node.name,
            egui::FontId::proportional(13.0),
            egui::Color32::WHITE,
        );

        for (i, port) in node.inputs.iter().enumerate() {
            let p = egui::pos2(nrect.left(), nrect.top() + 28.0 + i as f32 * 14.0);
            port_positions.insert((node_id, port.id), p);
            let color = match port.port_type {
                PortType::Midi => egui::Color32::from_rgb(220, 180, 80),
                PortType::Audio => egui::Color32::from_rgb(80, 180, 220),
            };
            painter.circle_filled(p, 5.0, color);
        }
        for (i, port) in node.outputs.iter().enumerate() {
            let p = egui::pos2(nrect.right(), nrect.top() + 28.0 + i as f32 * 14.0);
            port_positions.insert((node_id, port.id), p);
            let color = match port.port_type {
                PortType::Midi => egui::Color32::from_rgb(220, 180, 80),
                PortType::Audio => egui::Color32::from_rgb(80, 180, 220),
            };
            painter.circle_filled(p, 5.0, color);
        }
    }

    // Drive drag from primary button + app state (not egui's dragged()/drag_stopped()).
    let primary_down = ui.input(|i| i.pointer.primary_down());
    let primary_pressed = ui.input(|i| i.pointer.primary_pressed());
    let primary_released = ui.input(|i| i.pointer.primary_released());

    if primary_pressed {
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
                node.position[0] += delta.x;
                node.position[1] += delta.y;
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
            let graph_position = [
                pos.x - rect.left() - pan.x,
                pos.y - rect.top() - pan.y,
            ];
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
