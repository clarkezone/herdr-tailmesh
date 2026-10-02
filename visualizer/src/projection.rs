//! Pure, scoped projection of complete replacement snapshots.
use crate::pb::{HerdrEntity, HerdrState, NodeList};
use crate::{
    heartbeat::{Heartbeat, Stamp},
    summary::{ContextCounts, Counts, NodeCounts},
};
use prost_types::Timestamp;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

pub type Key = Vec<String>;
// Match the existing Go inventory bounds before cloning scoped identity keys.
const MAX_NODES: usize = 128;
const MAX_SESSIONS: usize = 64;
const MAX_ENTITIES: usize = 4096;
const MAX_ID_BYTES: usize = 128;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freshness {
    Receipt(Stamp),
    Stale,
    Unknown,
    Disconnected,
}
impl Freshness {
    pub fn is_live(self, time: Stamp) -> bool {
        let stamp = match self {
            Self::Receipt(t) => t,
            _ => return false,
        };
        (stamp.nanos() - time.nanos()).abs() < 30_000_000_000
    }
    pub fn label(self, now: i64) -> &'static str {
        self.label_at(Stamp::seconds(now))
    }
    pub fn label_at(self, time: Stamp) -> &'static str {
        match self {
            Self::Receipt(t) if t.nanos() - time.nanos() >= 30_000_000_000 => {
                "freshness unknown (clock ahead)"
            }
            Self::Receipt(_) if !self.is_live(time) => "stale",
            Self::Receipt(_) => "live",
            Self::Stale => "stale",
            Self::Unknown => "freshness unknown",
            Self::Disconnected => "disconnected",
        }
    }
}
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
pub fn wall_now() -> Stamp {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    Stamp {
        seconds: time.as_secs() as i64,
        nanos: time.subsec_nanos() as i32,
    }
}
fn receipt(t: Option<&Timestamp>, connected: bool, stale: bool) -> Freshness {
    if !connected {
        return Freshness::Disconnected;
    }
    if stale {
        return Freshness::Stale;
    }
    t.and_then(Stamp::parse)
        .map_or(Freshness::Unknown, Freshness::Receipt)
}
pub fn label(s: &str) -> String {
    bounded_text(s, 160)
}
fn detail(s: &str) -> String {
    bounded_text(s, 4096)
}
fn bounded_text(s: &str, limit: usize) -> String {
    let mut chars = s.chars().filter(|c| !c.is_control());
    let mut out: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        out.push('…');
    }
    out
}
fn named(display: &str, id: &str) -> String {
    label(if display.is_empty() { id } else { display })
}
fn key(parent: &Key, kind: &str, id: &str) -> Key {
    let mut k = parent.clone();
    k.push(kind.into());
    k.push(id.into());
    k
}

#[derive(Debug)]
pub struct Branch {
    pub key: Key,
    pub kind: &'static str,
    pub label: String,
    /// Reported workspace affiliation, not a repository identity inferred by the viewer.
    pub project_id: Option<String>,
    pub details: Vec<String>,
    pub status: String,
    pub freshness: Freshness,
    pub last_seen: Option<Stamp>,
    pub children: Vec<Branch>,
}
fn branch(
    parent: &Key,
    kind: &'static str,
    id: &str,
    display: &str,
    freshness: Freshness,
) -> Branch {
    Branch {
        key: key(parent, kind, id),
        kind,
        label: named(display, id),
        project_id: None,
        details: vec![
            format!("{kind} ID: {}", detail(id)),
            format!(
                "Label: {}",
                detail(if display.is_empty() { id } else { display })
            ),
        ],
        status: String::new(),
        freshness,
        last_seen: None,
        children: vec![],
    }
}
#[derive(Debug, Default)]
pub struct Scene {
    pub coordinator: Option<Branch>,
    pub nodes: Vec<Branch>,
    pub heartbeats: Vec<Heartbeat>,
    pub counts: Vec<NodeCounts>,
    pub observed_at: Stamp,
    pub projects: Vec<ProjectSummary>,
    pub connected: usize,
    pub agents: usize,
    pub working: usize,
    pub blocked: usize,
}

pub fn coordinator(info: Option<&crate::pb::ServerInfo>) -> Branch {
    let verified = info.filter(|i| {
        !i.instance_id.is_empty()
            && i.instance_id.len() <= MAX_ID_BYTES
            && !i.instance_id.chars().any(char::is_control)
    });
    let mut root = branch(
        &vec![],
        "coordinator",
        verified.map_or("unknown", |i| i.instance_id.as_str()),
        verified.map_or("Identity unknown", |i| i.instance_id.as_str()),
        Freshness::Unknown,
    );
    root.status = if verified.is_some() {
        "verified upstream identity"
    } else {
        "identity unavailable"
    }
    .into();
    if let Some(info) = verified {
        root.details.push(format!(
            "Coordinator version: {}",
            label(&info.implementation_version)
        ));
    }
    root.details
        .push("Logical control root; execution role is counted separately.".into());
    root
}

fn context_counts(session: &Branch, state: Option<&HerdrState>) -> ContextCounts {
    let mut inventory = Counts::default();
    if let Some(state) = state {
        inventory.workspaces = state.workspaces.len();
        inventory.agents = state.agents.len();
        for agent in &state.agents {
            inventory.working += usize::from(agent.agent_status == "working");
            inventory.blocked += usize::from(agent.agent_status == "blocked");
            inventory.done += usize::from(agent.agent_status == "done");
        }
    }
    ContextCounts {
        freshness: session.freshness,
        inventory,
    }
}

#[derive(Debug)]
pub struct ProjectSummary {
    pub id: String,
    pub workspaces: usize,
    pub nodes: usize,
}

pub fn project(snapshot: NodeList) -> Result<Scene, String> {
    if snapshot.nodes.len() > MAX_NODES {
        return Err("Node inventory exceeds the protocol bound".into());
    }
    let mut scene = Scene {
        observed_at: wall_now(),
        ..Default::default()
    };
    let mut ids = HashSet::new();
    for n in snapshot.nodes {
        if n.instance_id.is_empty()
            || n.instance_id.len() > MAX_ID_BYTES
            || !ids.insert(n.instance_id.clone())
        {
            return Err("Duplicate or missing node identity".into());
        }
        if n.sessions.len() > MAX_SESSIONS {
            return Err("Session inventory exceeds the protocol bound".into());
        }
        scene.connected += usize::from(n.connected);
        let mut node = branch(
            &vec![],
            "node",
            &n.instance_id,
            &n.hostname,
            receipt(n.last_seen.as_ref(), n.connected, false),
        );
        node.status = if n.connected { "connected" } else { "offline" }.into();
        node.last_seen = n.last_seen.as_ref().and_then(Stamp::parse);
        scene.heartbeats.push(Heartbeat {
            key: node.key.clone(),
            connected: n.connected,
            seen: node.last_seen,
        });
        let mut counts = NodeCounts {
            connected: n.connected,
            contexts: vec![],
        };
        node.details.extend([
            format!("Tailscale stable ID: {}", label(&n.tailscale_stable_id)),
            format!(
                "Readiness: commands {}, workspaces {}, worktrees {}, agents {}, sessions {}",
                n.command_ready,
                n.workspace_ready,
                n.worktree_ready,
                n.agent_ready,
                n.sessions_ready
            ),
            format!("Session discovery error: {}", label(&n.sessions_error_code)),
        ]);
        let discovered = n.sessions_ready
            || n.sessions_received_at.is_some()
            || !n.sessions_error_code.is_empty()
            || !n.sessions.is_empty();
        let mut sessions = HashSet::new();
        if n.herdr.as_ref().is_some_and(|h| h.status != "disabled") || !discovered {
            let mut session = branch(
                &node.key,
                "session",
                "",
                "Configured default",
                receipt(n.herdr_received_at.as_ref(), n.connected, n.stale),
            );
            session.key.push(String::new()); // explicit incarnation component
            populate(&mut session, n.herdr.as_ref(), &mut scene)?;
            counts
                .contexts
                .push(context_counts(&session, n.herdr.as_ref()));
            node.children.push(session);
        }
        for s in n.sessions {
            if s.name.is_empty()
                || s.name.len() > 64
                || s.incarnation.len() > 64
                || !sessions.insert((s.name.clone(), s.incarnation.clone()))
            {
                return Err("Duplicate or missing session identity".into());
            }
            let mut session = branch(
                &node.key,
                "session",
                &s.name,
                &s.name,
                receipt(s.herdr_received_at.as_ref(), n.connected, s.stale),
            );
            session.key.push(s.incarnation.clone());
            if s.status != "ready" && matches!(session.freshness, Freshness::Receipt(_)) {
                session.freshness = Freshness::Unknown;
            }
            session.details.push(format!(
                "Incarnation: {} · {} · {}",
                label(&s.incarnation),
                label(&s.status),
                label(&s.error_code)
            ));
            populate(&mut session, s.herdr.as_ref(), &mut scene)?;
            if s.status != "ready" {
                session.status = label(&s.status);
            }
            counts
                .contexts
                .push(context_counts(&session, s.herdr.as_ref()));
            node.children.push(session);
        }
        sort(&mut node.children);
        scene.nodes.push(node);
        scene.counts.push(counts);
    }
    sort(&mut scene.nodes);
    // Count scoped workspace observations once; a node with several sessions
    // contributes only once to a project's node count. Do not guess from agents.
    let mut projects: BTreeMap<String, (usize, HashSet<&Key>)> = BTreeMap::new();
    for node in &scene.nodes {
        for session in &node.children {
            for workspace in &session.children {
                if let Some(id) = workspace.project_id.as_ref().filter(|id| !id.is_empty()) {
                    let entry = projects.entry(id.clone()).or_default();
                    entry.0 += 1;
                    entry.1.insert(&node.key);
                }
            }
        }
    }
    scene.projects = projects
        .into_iter()
        .map(|(id, (workspaces, nodes))| ProjectSummary {
            id,
            workspaces,
            nodes: nodes.len(),
        })
        .collect();
    Ok(scene)
}

fn unique(entities: &[HerdrEntity], scoped: bool) -> Result<(), String> {
    let mut seen = HashSet::new();
    for e in entities {
        if [&e.id, &e.workspace_id, &e.tab_id, &e.project_id]
            .iter()
            .any(|id| id.len() > MAX_ID_BYTES)
        {
            return Err("Entity identity exceeds the protocol bound".into());
        }
        let k = if scoped {
            vec![e.workspace_id.clone(), e.tab_id.clone(), e.id.clone()]
        } else {
            vec![e.id.clone()]
        };
        if e.id.is_empty() || !seen.insert(k) {
            return Err("Duplicate or missing entity identity".into());
        }
    }
    Ok(())
}
fn sort(branches: &mut [Branch]) {
    branches.sort_by(|a, b| (&a.label, &a.key).cmp(&(&b.label, &b.key)));
}
fn aggregate(children: &[Branch]) -> String {
    for status in ["blocked", "working", "unknown", "idle", "done"] {
        if children.iter().any(|c| c.status == status) {
            return status.into();
        }
    }
    "no observed agents".into()
}
fn populate(
    session: &mut Branch,
    state: Option<&HerdrState>,
    scene: &mut Scene,
) -> Result<(), String> {
    let Some(h) = state else {
        session.status = "no observation".into();
        if matches!(session.freshness, Freshness::Receipt(_)) {
            session.freshness = Freshness::Unknown;
        }
        return Ok(());
    };
    if h.status != "ready" && matches!(session.freshness, Freshness::Receipt(_)) {
        session.freshness = Freshness::Unknown;
    }
    session.status = label(&h.status);
    session.details.push(format!(
        "Herdr {} · version {} · error {}",
        label(&h.status),
        label(&h.version),
        label(&h.error_code)
    ));
    if h.workspaces.len() + h.tabs.len() + h.panes.len() + h.agents.len() > MAX_ENTITIES {
        return Err("Entity inventory exceeds the protocol bound".into());
    }
    unique(&h.workspaces, false)?;
    unique(&h.tabs, true)?;
    unique(&h.panes, true)?;
    unique(&h.agents, true)?;
    // Index joins once; repeated linear searches can freeze large snapshots.
    let panes: HashMap<_, _> = h
        .panes
        .iter()
        .map(|p| {
            (
                (p.workspace_id.as_str(), p.tab_id.as_str(), p.id.as_str()),
                p,
            )
        })
        .collect();
    let tabs: HashMap<_, _> = h
        .tabs
        .iter()
        .map(|t| ((t.workspace_id.as_str(), t.id.as_str()), t))
        .collect();
    let mut workspaces: BTreeMap<String, Branch> = BTreeMap::new();
    let mut project_ids = BTreeMap::new();
    for w in &h.workspaces {
        let mut workspace = branch(
            &session.key,
            "workspace",
            &w.id,
            &w.display_name,
            session.freshness,
        );
        workspace
            .details
            .push(format!("Directory: {}", detail(&w.directory)));
        workspace
            .details
            .push(format!("Reported project: {}", detail(&w.project_id)));
        workspace.project_id = Some(w.project_id.clone());
        project_ids.insert(w.id.clone(), w.project_id.clone());
        workspaces.insert(w.id.clone(), workspace);
    }
    for a in &h.agents {
        scene.agents += 1;
        scene.working += usize::from(a.agent_status == "working");
        scene.blocked += usize::from(a.agent_status == "blocked");
        let workspace = workspaces.entry(a.workspace_id.clone()).or_insert_with(|| {
            let mut w = branch(
                &session.key,
                "workspace",
                &a.workspace_id,
                "Unresolved workspace",
                session.freshness,
            );
            w.details
                .push("Workspace reference is absent from this snapshot".into());
            w.project_id = Some(String::new());
            w
        });
        let mut parent = workspace.key.clone();
        parent.push(a.tab_id.clone());
        let pane = panes.get(&(a.workspace_id.as_str(), a.tab_id.as_str(), a.id.as_str()));
        let tab = tabs.get(&(a.workspace_id.as_str(), a.tab_id.as_str()));
        let display = if !a.display_name.is_empty() {
            &a.display_name
        } else {
            pane.map_or("", |p| p.display_name.as_str())
        };
        let mut agent = branch(&parent, "agent", &a.id, display, session.freshness);
        agent.status = if ["working", "blocked", "done", "idle"].contains(&a.agent_status.as_str())
        {
            a.agent_status.clone()
        } else {
            "unknown".into()
        };
        let inherited = project_ids.get(&a.workspace_id).map_or("", String::as_str);
        let affiliation = if a.project_id.is_empty() {
            inherited
        } else {
            &a.project_id
        };
        agent.details.extend([
            format!(
                "Workspace: {} · Tab: {} ({})",
                label(&a.workspace_id),
                label(&a.tab_id),
                tab.map_or_else(|| "unresolved".into(), |t| named(&t.display_name, &t.id))
            ),
            format!(
                "Provider: {} · Interactive ready: {}",
                label(&a.provider),
                a.interactive_ready
                    .map_or("unknown", |r| if r { "yes" } else { "no" })
            ),
            format!(
                "Terminal: {} · Provider session: {}",
                label(&a.terminal_id),
                label(&a.provider_session_id)
            ),
            format!(
                "Directory: {} · Focused: {}",
                detail(&a.directory),
                a.focused
            ),
            format!("Reported project: {}", detail(affiliation)),
        ]);
        if !a.project_id.is_empty() && !inherited.is_empty() && a.project_id != inherited {
            agent.details.push(
                "Project affiliation differs from workspace; workspace owns placement".into(),
            );
        }
        workspace.children.push(agent);
    }
    for (_, mut w) in workspaces {
        sort(&mut w.children);
        w.status = aggregate(&w.children);
        session.children.push(w);
    }
    sort(&mut session.children);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pb::{NodeView, SessionView};
    fn entity(id: &str) -> HerdrEntity {
        HerdrEntity {
            id: id.into(),
            ..Default::default()
        }
    }
    #[test]
    fn scoped_sessions_and_unresolved_placement() {
        let mut w = entity("w");
        w.project_id = "workspace-project".into();
        let mut a = entity("a");
        a.workspace_id = "w".into();
        a.project_id = "agent-project".into();
        a.agent_status = "working".into();
        let mut orphan = entity("orphan");
        orphan.workspace_id = "missing".into();
        let h = HerdrState {
            status: "ready".into(),
            workspaces: vec![w],
            agents: vec![a, orphan],
            ..Default::default()
        };
        let n = NodeView {
            instance_id: "n".into(),
            connected: true,
            sessions_ready: true,
            herdr: Some(HerdrState {
                status: "disabled".into(),
                ..Default::default()
            }),
            sessions: vec![
                SessionView {
                    name: "one".into(),
                    incarnation: "1".into(),
                    herdr: Some(h.clone()),
                    ..Default::default()
                },
                SessionView {
                    name: "two".into(),
                    incarnation: "2".into(),
                    herdr: Some(h),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let s = project(NodeList { nodes: vec![n] }).unwrap();
        assert_eq!(s.nodes[0].children.len(), 2);
        assert_eq!(s.agents, 4);
        assert_eq!(s.working, 2);
        assert_ne!(s.nodes[0].children[0].key, s.nodes[0].children[1].key);
        let p = s.nodes[0].children[0]
            .children
            .iter()
            .find(|p| p.project_id.as_deref() == Some("workspace-project"))
            .unwrap();
        assert!(p.children[0].details.iter().any(|d| d.contains("differs")));
        assert_eq!(p.kind, "workspace");
        assert_eq!(s.projects.len(), 1);
        assert_eq!(s.projects[0].id, "workspace-project");
        assert_eq!(s.projects[0].workspaces, 2);
        assert_eq!(s.projects[0].nodes, 1);
    }
    #[test]
    fn rejects_duplicates_and_safe_labels() {
        let n = NodeView {
            instance_id: "n".into(),
            ..Default::default()
        };
        assert!(
            project(NodeList {
                nodes: vec![n.clone(), n]
            })
            .is_err()
        );
        assert_eq!(label("a\nb\0c"), "abc");
        assert_eq!(label(&"🦀".repeat(200)).chars().count(), 161);
        assert_eq!(detail(&"🦀".repeat(200)).chars().count(), 200);
        assert_eq!(detail(&"🦀".repeat(5000)).chars().count(), 4097);
    }

    #[test]
    fn rejects_identity_and_inventory_amplification_before_building_scoped_keys() {
        let node = |id: &str, state| NodeView {
            instance_id: id.into(),
            herdr: state,
            ..Default::default()
        };
        assert!(
            project(NodeList {
                nodes: vec![node(&"x".repeat(129), None)]
            })
            .is_err()
        );
        assert!(
            project(NodeList {
                nodes: (0..129).map(|i| node(&format!("node-{i}"), None)).collect()
            })
            .is_err()
        );
        for field in ["id", "workspace", "tab", "project"] {
            let mut invalid = entity("agent");
            match field {
                "id" => invalid.id = "x".repeat(129),
                "workspace" => invalid.workspace_id = "x".repeat(129),
                "tab" => invalid.tab_id = "x".repeat(129),
                _ => invalid.project_id = "x".repeat(129),
            }
            let state = HerdrState {
                status: "ready".into(),
                agents: vec![invalid],
                ..Default::default()
            };
            assert!(
                project(NodeList {
                    nodes: vec![node("node", Some(state))]
                })
                .is_err()
            );
        }
        let state = HerdrState {
            status: "ready".into(),
            workspaces: (0..4097)
                .map(|i| entity(&format!("workspace-{i}")))
                .collect(),
            ..Default::default()
        };
        assert!(
            project(NodeList {
                nodes: vec![node("node", Some(state))]
            })
            .is_err()
        );
        let mut n = node("node", None);
        n.sessions = (0..65)
            .map(|i| SessionView {
                name: format!("session-{i}"),
                ..Default::default()
            })
            .collect();
        assert!(project(NodeList { nodes: vec![n] }).is_err());
    }
    #[test]
    fn project_index_counts_workspaces_and_distinct_nodes_without_guessing_identity() {
        let mut one = entity("one");
        one.project_id = "shared".into();
        let mut two = entity("two");
        two.project_id = "shared".into();
        let mut fork = entity("fork");
        fork.project_id = "separate".into();
        let node = |id: &str, workspaces: Vec<HerdrEntity>| NodeView {
            instance_id: id.into(),
            herdr: Some(HerdrState {
                status: "ready".into(),
                workspaces,
                ..Default::default()
            }),
            ..Default::default()
        };
        let scene = project(NodeList {
            nodes: vec![
                node("first", vec![one.clone(), two, entity("unassigned"), fork]),
                node("second", vec![one]),
            ],
        })
        .unwrap();
        assert_eq!(scene.projects.len(), 2);
        let shared = scene.projects.iter().find(|p| p.id == "shared").unwrap();
        assert_eq!((shared.workspaces, shared.nodes), (3, 2));
        let before = &scene.nodes[0].children[0].children;
        assert!(before.iter().all(|w| w.kind == "workspace"));
        let original_key = before
            .iter()
            .find(|w| w.project_id.as_deref() == Some("shared"))
            .unwrap()
            .key
            .clone();
        let mut renamed = entity(original_key.last().unwrap());
        renamed.project_id = "changed".into();
        let replacement = project(NodeList {
            nodes: vec![node("first", vec![renamed])],
        })
        .unwrap();
        assert_eq!(
            replacement.nodes[0].children[0].children[0].key, original_key,
            "changing affiliation must not replace workspace identity"
        );
    }
    #[test]
    fn freshness_is_receipt_based() {
        assert_eq!(Freshness::Receipt(Stamp::seconds(100)).label(129), "live");
        assert_eq!(Freshness::Receipt(Stamp::seconds(100)).label(130), "stale");
        assert!(
            Freshness::Receipt(Stamp::seconds(200))
                .label(100)
                .contains("clock")
        );
        assert_eq!(receipt(None, true, false), Freshness::Unknown);
        assert_eq!(receipt(None, false, false), Freshness::Disconnected);
    }
    #[test]
    fn unavailable_session_keeps_recent_agent_observations_unknown() {
        let h = HerdrState {
            status: "unavailable".into(),
            agents: vec![HerdrEntity {
                id: "agent".into(),
                agent_status: "working".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let n = NodeView {
            instance_id: "node".into(),
            connected: true,
            herdr: Some(h),
            herdr_received_at: Some(Timestamp {
                seconds: now(),
                nanos: 0,
            }),
            ..Default::default()
        };
        let scene = project(NodeList { nodes: vec![n] }).unwrap();
        let session = &scene.nodes[0].children[0];
        assert_eq!(session.freshness, Freshness::Unknown);
        assert_eq!(
            session.children[0].children[0].freshness,
            Freshness::Unknown
        );
        assert_eq!(
            scene.working, 1,
            "retained observation remains counted as last-known"
        );
    }
}
