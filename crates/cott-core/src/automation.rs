//! Parameter automation lanes and interpolation.

use crate::ids::{AutomationLaneId, NodeId, PluginInstanceId};
use serde::{Deserialize, Serialize};

/// Beat-position equality tolerance shared by point mutation and lookup.
const BEAT_EPSILON: f64 = 1e-9;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AutomationTarget {
    NodeGain {
        node_id: NodeId,
    },
    NodePan {
        node_id: NodeId,
    },
    PluginParam {
        instance_id: PluginInstanceId,
        param_id: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationPoint {
    /// Timeline position in beats.
    pub beat: f64,
    /// Normalized 0..1 for plugin params; gain uses linear gain mapped elsewhere.
    pub value: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationLane {
    pub id: AutomationLaneId,
    pub target: AutomationTarget,
    pub points: Vec<AutomationPoint>,
    pub enabled: bool,
}

impl AutomationLane {
    pub fn new(target: AutomationTarget) -> Self {
        Self {
            id: AutomationLaneId::new(),
            target,
            points: Vec::new(),
            enabled: true,
        }
    }

    pub fn add_point(&mut self, beat: f64, value: f32) {
        let value = value.clamp(0.0, 1.0);
        if let Some(existing) = self
            .points
            .iter_mut()
            .find(|p| (p.beat - beat).abs() < BEAT_EPSILON)
        {
            existing.value = value;
        } else {
            self.points.push(AutomationPoint { beat, value });
            self.points.sort_by(|a, b| {
                a.beat
                    .partial_cmp(&b.beat)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
    }

    pub fn remove_point_at(&mut self, beat: f64) {
        self.points
            .retain(|p| (p.beat - beat).abs() >= BEAT_EPSILON);
    }

    /// Piecewise-linear interpolation. Holds first/last outside range.
    pub fn value_at(&self, beat: f64) -> f32 {
        if self.points.is_empty() {
            return 0.5;
        }
        if beat <= self.points[0].beat + BEAT_EPSILON {
            return self.points[0].value;
        }
        let last = self.points.last().unwrap();
        if beat >= last.beat - BEAT_EPSILON {
            return last.value;
        }
        for window in self.points.windows(2) {
            let a = &window[0];
            let b = &window[1];
            // Same tolerance as add_point/remove_point_at so near-boundary
            // queries stay consistent with point identity.
            if beat >= a.beat - BEAT_EPSILON && beat <= b.beat + BEAT_EPSILON {
                let span = (b.beat - a.beat).max(BEAT_EPSILON);
                let t = ((beat - a.beat) / span) as f32;
                return a.value + (b.value - a.value) * t.clamp(0.0, 1.0);
            }
        }
        last.value
    }
}

/// Map normalized 0..1 to gain dB in [-60, +12].
pub fn normalized_to_gain_db(n: f32) -> f32 {
    -60.0 + n.clamp(0.0, 1.0) * 72.0
}

pub fn gain_db_to_normalized(db: f32) -> f32 {
    ((db + 60.0) / 72.0).clamp(0.0, 1.0)
}

pub fn gain_db_to_linear(db: f32) -> f32 {
    if db <= -60.0 {
        0.0
    } else {
        10f32.powf(db / 20.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_interpolation() {
        let mut lane = AutomationLane::new(AutomationTarget::NodePan {
            node_id: NodeId::new(),
        });
        lane.add_point(0.0, 0.0);
        lane.add_point(4.0, 1.0);
        assert!((lane.value_at(2.0) - 0.5).abs() < 1e-5);
        assert!((lane.value_at(-1.0) - 0.0).abs() < 1e-5);
        assert!((lane.value_at(10.0) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn value_at_uses_same_tolerance_as_add_point() {
        let mut lane = AutomationLane::new(AutomationTarget::NodePan {
            node_id: NodeId::new(),
        });
        lane.add_point(0.0, 0.0);
        lane.add_point(1.0, 1.0);
        lane.add_point(1.0 + 5e-10, 0.25);
        assert_eq!(lane.points.len(), 2);
        assert!((lane.value_at(1.0) - 0.25).abs() < 1e-5);
        assert!((lane.value_at(1.0 + 5e-10) - 0.25).abs() < 1e-5);
        assert!((lane.value_at(0.5) - 0.125).abs() < 1e-5);
    }
}
