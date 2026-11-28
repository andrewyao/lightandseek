//! What the chrome shows, as plain values derived from a `ViewState`: the
//! screen, the camera mode, the HUD, and the lobby rules.

use game::rules::{HUNGRY, MAX_BOTS, MAX_HUMANS};
use game::{
    AvatarView, Cause, Control, Link, LobbyConfig, Member, Phase, PlayerId, Refusal, Role, Status,
    ViewState,
};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Screen {
    Connecting,
    Lobby,
    Game,
    Results,
}

impl Screen {
    /// `None` is a socket that has not been welcomed yet.
    pub fn of(view: Option<&ViewState<'_>>) -> Screen {
        match view.map(|v| v.phase) {
            None => Screen::Connecting,
            Some(Phase::Lobby) => Screen::Lobby,
            Some(Phase::Hiding { .. } | Phase::Seeking { .. }) => Screen::Game,
            Some(Phase::Results { .. }) => Screen::Results,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Camera {
    /// Behind your own avatar.
    Follow,
    /// Circling someone else, because you are caught or not playing.
    Spectate(PlayerId),
}

/// Players still in the round other than you, by id.
fn watchable(view: &ViewState<'_>) -> Vec<PlayerId> {
    let mut ids: Vec<PlayerId> = view
        .others
        .iter()
        .filter(|a| a.status == Status::Active)
        .map(|a| a.id)
        .collect();
    ids.sort();
    ids
}

impl Camera {
    /// Follow while you are active. Otherwise keep watching the current
    /// target while it is still active, else the first active player.
    pub fn resolve(self, view: &ViewState<'_>) -> Camera {
        if view.me.is_some_and(|m| m.status == Status::Active) {
            return Camera::Follow;
        }
        let ids = watchable(view);
        match self {
            Camera::Spectate(t) if ids.contains(&t) => self,
            _ => ids.first().map_or(self, |&t| Camera::Spectate(t)),
        }
    }

    /// Tab: the next active player after the current target.
    pub fn cycle(self, view: &ViewState<'_>) -> Camera {
        let Camera::Spectate(t) = self.resolve(view) else {
            return Camera::Follow;
        };
        let ids = watchable(view);
        let next = ids.iter().find(|&&id| id > t).or(ids.first());
        next.map_or(self, |&id| Camera::Spectate(id))
    }
}

pub fn name_of(roster: &[Member], id: PlayerId) -> String {
    roster
        .iter()
        .find(|m| m.id == id)
        .map_or_else(|| format!("Player {}", id.0), |m| m.name.clone())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hud {
    pub role: String,
    pub phase: &'static str,
    /// Whole seconds left in the phase, rounded up.
    pub seconds: u32,
    /// `None` while blind: the seeker's snapshots carry no hiders then.
    pub hiders_left: Option<usize>,
    /// The seeker's screen is black while hiders hide.
    pub blind: bool,
    /// Whom a spectator is watching.
    pub watching: Option<String>,
    /// Your food and water in whole percent, while you are a living hider.
    pub meters: Option<Meters>,
    /// Whole seconds before the danger zone kills you, while you stand in it.
    pub danger: Option<u32>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Meters {
    pub food: u8,
    pub water: u8,
    /// Low food slows you.
    pub hungry: bool,
    pub thirsty: bool,
}

/// Below this share of water the meter warns.
const THIRSTY: f32 = 0.3;

fn percent(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 100.0).ceil() as u8
}

/// The toast when `name` dies of `cause`.
pub fn death(name: &str, cause: Cause) -> String {
    match cause {
        Cause::Thirst => format!("{name} died of thirst"),
        Cause::Zone => format!("{name} was lost to the danger zone"),
    }
}

pub fn hud(view: &ViewState<'_>, camera: Camera) -> Hud {
    let role = match view.me {
        Some(m) if m.status == Status::Caught => "Caught, spectating".into(),
        Some(AvatarView {
            status: Status::Died(Cause::Thirst),
            ..
        }) => "Died of thirst, spectating".into(),
        Some(AvatarView {
            status: Status::Died(Cause::Zone),
            ..
        }) => "Lost to the zone, spectating".into(),
        Some(m) if m.role == Role::Seeker => "Seeker".into(),
        Some(_) => "Hider".into(),
        None => "Spectating".into(),
    };
    let phase = match view.phase {
        Phase::Lobby => "Lobby",
        Phase::Hiding { .. } => "Hiding",
        Phase::Seeking { .. } => "Seeking",
        Phase::Results { .. } => "Results",
    };
    let watching = match camera {
        Camera::Spectate(id) => Some(name_of(view.roster, id)),
        Camera::Follow => None,
    };
    Hud {
        role,
        phase,
        seconds: view.seconds_left.ceil() as u32,
        hiders_left: (!view.blind()).then_some(view.hiders_left),
        blind: view.blind(),
        watching,
        meters: view.vitals.map(|v| Meters {
            food: percent(v.food),
            water: percent(v.water),
            hungry: v.food < HUNGRY,
            thirsty: v.water < THIRSTY,
        }),
        danger: view
            .danger
            .then(|| view.vitals.map_or(0.0, |v| v.danger_left()).ceil() as u32),
    }
}

pub fn winner(role: Role) -> &'static str {
    match role {
        Role::Seeker => "The seeker wins",
        Role::Hider => "The hiders win",
    }
}

pub fn refusal(why: Refusal) -> String {
    match why {
        Refusal::NotHost => "Only the host can change the game.".into(),
        Refusal::NotInLobby => "A round is already running.".into(),
        Refusal::BadConfig => "Those settings are out of range.".into(),
        Refusal::HumansMissing { have, want } => {
            format!("Waiting for players: {have} of {want} humans are here.")
        }
        Refusal::TooFewPlayers => "A game needs at least 2 players.".into(),
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Count {
    Humans,
    Bots,
}

/// `config` with one more or one fewer of `count`, or `None` when the
/// rules forbid it: 1 to `MAX_HUMANS` humans, 0 to `MAX_BOTS` bots, and at
/// least 2 players in all.
pub fn stepped(config: LobbyConfig, count: Count, up: bool) -> Option<LobbyConfig> {
    let (value, max) = match count {
        Count::Humans => (config.humans, MAX_HUMANS),
        Count::Bots => (config.bots, MAX_BOTS),
    };
    let value = if up {
        value.checked_add(1).filter(|v| *v <= max)?
    } else {
        value.checked_sub(1)?
    };
    let next = match count {
        Count::Humans => LobbyConfig {
            humans: value,
            ..config
        },
        Count::Bots => LobbyConfig {
            bots: value,
            ..config
        },
    };
    (next.humans >= 1 && next.humans + next.bots >= 2).then_some(next)
}

/// Humans connected now.
pub fn humans_here(roster: &[Member]) -> u8 {
    let n = roster
        .iter()
        .filter(|m| m.control == Control::Human && m.link == Link::Online)
        .count();
    n.min(u8::MAX as usize) as u8
}

/// Why Start would be refused, judged from what this client knows.
pub fn start_blocked(config: LobbyConfig, roster: &[Member]) -> Option<Refusal> {
    let have = humans_here(roster);
    if have < config.humans {
        Some(Refusal::HumansMissing {
            have,
            want: config.humans,
        })
    } else if config.humans + config.bots < 2 {
        Some(Refusal::TooFewPlayers)
    } else {
        None
    }
}

/// A roster line: the name and what is special about the member.
pub fn roster_line(m: &Member, you: PlayerId, host: Option<PlayerId>) -> String {
    let mut tags = Vec::new();
    if m.id == you {
        tags.push("you");
    }
    if Some(m.id) == host {
        tags.push("host");
    }
    if m.control == Control::Bot {
        tags.push("bot");
    }
    if m.link == Link::Away {
        tags.push("away");
    }
    if m.seat == game::Seat::Spectating {
        tags.push("spectating");
    }
    if tags.is_empty() {
        m.name.clone()
    } else {
        format!("{} ({})", m.name, tags.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use game::{AvatarView, Body, Seat, Tick, Vec2};

    use super::*;

    fn member(id: u32, control: Control, link: Link) -> Member {
        Member {
            id: PlayerId(id),
            name: format!("p{id}"),
            control,
            seat: Seat::Playing,
            link,
        }
    }

    fn avatar(id: u32, role: Role, status: Status) -> AvatarView {
        AvatarView {
            id: PlayerId(id),
            body: Body {
                pos: Vec2::new(id as f32, 0.0),
                ..Body::default()
            },
            role,
            status,
            control: Control::Human,
        }
    }

    fn view<'a>(
        roster: &'a [Member],
        phase: Phase,
        me: Option<AvatarView>,
        others: Vec<AvatarView>,
    ) -> ViewState<'a> {
        ViewState {
            you: PlayerId(1),
            host: Some(PlayerId(1)),
            config: LobbyConfig::default(),
            roster,
            phase,
            world: None,
            seeker: Some(PlayerId(2)),
            me,
            others,
            hiders_left: 2,
            seconds_left: 2.2,
            zone: None,
            food: Vec::new(),
            vitals: None,
            danger: false,
        }
    }

    const SEEKING: Phase = Phase::Seeking { until: Tick(100) };

    #[test]
    fn the_screen_follows_the_phase() {
        let roster = [];
        assert_eq!(Screen::of(None), Screen::Connecting);
        let at = |phase| Screen::of(Some(&view(&roster, phase, None, Vec::new())));
        assert_eq!(at(Phase::Lobby), Screen::Lobby);
        assert_eq!(at(Phase::Hiding { until: Tick(9) }), Screen::Game);
        assert_eq!(at(SEEKING), Screen::Game);
        let results = Phase::Results {
            until: Tick(9),
            winner: Role::Hider,
        };
        assert_eq!(at(results), Screen::Results);
    }

    #[test]
    fn an_active_player_follows_and_a_caught_one_spectates_the_first_active() {
        let roster = [];
        let others = vec![
            avatar(4, Role::Hider, Status::Caught),
            avatar(3, Role::Hider, Status::Active),
            avatar(2, Role::Seeker, Status::Active),
        ];
        let me = avatar(1, Role::Hider, Status::Active);
        let v = view(&roster, SEEKING, Some(me), others.clone());
        assert_eq!(Camera::Spectate(PlayerId(3)).resolve(&v), Camera::Follow);

        let caught = AvatarView {
            status: Status::Caught,
            ..me
        };
        let v = view(&roster, SEEKING, Some(caught), others.clone());
        assert_eq!(Camera::Follow.resolve(&v), Camera::Spectate(PlayerId(2)));
        assert_eq!(
            Camera::Spectate(PlayerId(3)).resolve(&v),
            Camera::Spectate(PlayerId(3)),
            "keeps a target that is still active"
        );
        assert_eq!(
            Camera::Spectate(PlayerId(4)).resolve(&v),
            Camera::Spectate(PlayerId(2)),
            "drops a caught target"
        );

        let late = view(&roster, SEEKING, None, others);
        assert_eq!(Camera::Follow.resolve(&late), Camera::Spectate(PlayerId(2)));
    }

    #[test]
    fn tab_cycles_through_active_players_and_wraps() {
        let roster = [];
        let others = vec![
            avatar(2, Role::Seeker, Status::Active),
            avatar(3, Role::Hider, Status::Caught),
            avatar(5, Role::Hider, Status::Active),
        ];
        let v = view(&roster, SEEKING, None, others);
        let a = Camera::Follow.cycle(&v);
        assert_eq!(a, Camera::Spectate(PlayerId(5)));
        assert_eq!(a.cycle(&v), Camera::Spectate(PlayerId(2)));
        let playing = view(
            &roster,
            SEEKING,
            Some(avatar(1, Role::Hider, Status::Active)),
            Vec::new(),
        );
        assert_eq!(
            a.cycle(&playing),
            Camera::Follow,
            "no tabbing while playing"
        );
    }

    #[test]
    fn the_hud_names_role_phase_and_rounds_seconds_up() {
        let roster = [member(2, Control::Bot, Link::Online)];
        let seeker = avatar(1, Role::Seeker, Status::Active);
        let hiding = Phase::Hiding { until: Tick(9) };
        let h = hud(
            &view(&roster, hiding, Some(seeker), Vec::new()),
            Camera::Follow,
        );
        assert_eq!(
            (h.role.as_str(), h.phase, h.seconds, h.hiders_left, h.blind),
            ("Seeker", "Hiding", 3, None, true)
        );
        let caught = avatar(1, Role::Hider, Status::Caught);
        let h = hud(
            &view(&roster, SEEKING, Some(caught), Vec::new()),
            Camera::Spectate(PlayerId(2)),
        );
        assert_eq!(h.role, "Caught, spectating");
        assert_eq!(h.watching.as_deref(), Some("p2"));
        assert!(!h.blind);
    }

    #[test]
    fn a_living_hider_sees_its_meters_and_a_danger_countdown() {
        let roster = [member(2, Control::Bot, Link::Online)];
        let me = avatar(1, Role::Hider, Status::Active);
        let mut v = view(&roster, SEEKING, Some(me), Vec::new());
        v.vitals = Some(game::Vitals {
            food: 0.2,
            water: 0.505,
            danger: 0,
        });
        let h = hud(&v, Camera::Follow);
        assert_eq!(
            h.meters,
            Some(Meters {
                food: 20,
                water: 51,
                hungry: true,
                thirsty: false,
            })
        );
        assert_eq!(h.danger, None, "safe");

        v.danger = true;
        v.vitals = v.vitals.map(|x| game::Vitals { danger: 70, ..x });
        assert_eq!(hud(&v, Camera::Follow).danger, Some(7), "6.5 s rounds up");

        let mut v = view(&roster, SEEKING, Some(me), Vec::new());
        v.me = Some(AvatarView {
            status: Status::Died(Cause::Zone),
            ..me
        });
        let h = hud(&v, Camera::Follow);
        assert_eq!(h.role, "Lost to the zone, spectating");
        assert_eq!((h.meters, h.danger), (None, None));
        assert_eq!(death("Ada", Cause::Thirst), "Ada died of thirst");
    }

    #[test]
    fn steppers_keep_one_human_and_two_players() {
        let c = |humans, bots| LobbyConfig {
            humans,
            bots,
            ..LobbyConfig::default()
        };
        assert_eq!(stepped(c(1, 1), Count::Humans, false), None);
        assert_eq!(stepped(c(1, 1), Count::Bots, false), None);
        assert_eq!(stepped(c(2, 0), Count::Humans, false), None);
        assert_eq!(stepped(c(2, 1), Count::Humans, false), Some(c(1, 1)));
        assert_eq!(stepped(c(1, 1), Count::Bots, true), Some(c(1, 2)));
        assert_eq!(stepped(c(MAX_HUMANS, 0), Count::Humans, true), None);
        assert_eq!(stepped(c(1, MAX_BOTS), Count::Bots, true), None);
        assert_eq!(stepped(c(2, 0), Count::Bots, false), None);
    }

    #[test]
    fn start_waits_for_every_human_seat() {
        let roster = [
            member(1, Control::Human, Link::Online),
            member(2, Control::Human, Link::Away),
            member(3, Control::Bot, Link::Online),
        ];
        let config = LobbyConfig {
            humans: 2,
            bots: 2,
            ..LobbyConfig::default()
        };
        assert_eq!(
            start_blocked(config, &roster),
            Some(Refusal::HumansMissing { have: 1, want: 2 })
        );
        let one = LobbyConfig {
            humans: 1,
            ..config
        };
        assert_eq!(start_blocked(one, &roster), None);
        assert!(refusal(Refusal::HumansMissing { have: 1, want: 2 }).contains("1 of 2"));
    }

    #[test]
    fn roster_lines_tag_you_the_host_and_bots() {
        let m = member(1, Control::Human, Link::Online);
        assert_eq!(
            roster_line(&m, PlayerId(1), Some(PlayerId(1))),
            "p1 (you, host)"
        );
        let b = member(3, Control::Bot, Link::Online);
        assert_eq!(roster_line(&b, PlayerId(1), Some(PlayerId(1))), "p3 (bot)");
        let plain = member(2, Control::Human, Link::Online);
        assert_eq!(roster_line(&plain, PlayerId(1), None), "p2");
    }
}
