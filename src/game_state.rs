use std::{collections::HashMap, cmp::Ordering};

use anyhow::{bail, Error, anyhow};
use serde::{Deserialize, Serialize};

use crate::rng::Rng;

pub type PlayerToken = String;
pub type PlayerIndex = usize;
pub type TerritoryIndex = usize;

#[derive(Debug, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", tag = "kind")]
#[ts(export)]
pub enum GameAction {
  SetCommand {
    territory: TerritoryIndex,
    command:   Command,
  },
  Resign,
}

#[derive(Debug, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", tag = "kind")]
#[ts(export)]
pub enum Command {
  Attack { target: TerritoryIndex },
  Fortify,
  Grow,
}

#[derive(Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", tag = "kind")]
#[ts(export)]
pub struct PlayerState {
  pub is_alive:      bool,
  pub defense_level: i32,
  pub attack_level:  i32,
  pub vision_level:  i32,
  pub growth_level:  i32,
}

#[derive(Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", tag = "kind")]
#[ts(export)]
pub enum TerritorySort {
  /// Land is the default.
  Land,
  /// Units on swap territory get -1 as defenders.
  Swamp,
  /// Units on forest territory get +1 as defenders, and can't be seen except by adjacent units.
  Forest,
  /// Units on towers get +1 to vision range.
  Tower,
  /// Units on gold territory get +1 gold per turn.
  Gold,
  /// Units on lab territory give +1 research per turn.
  Lab,
}

#[derive(Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", tag = "kind")]
#[ts(export)]
pub struct Territory {
  pub sort:        TerritorySort,
  pub contents:    Option<(PlayerIndex, i32)>,
  pub command:     Command,
  pub adjacent:    Vec<TerritoryIndex>,
  pub render_info: (i32, i32),
}

#[derive(Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", tag = "kind")]
#[ts(export)]
pub enum AnimationEvent {
  Death {
    render_info: (i32, i32),
    amount:      i32,
  },
  Movement {
    render_info_from: (i32, i32),
    render_info_to:   (i32, i32),
    amount:           i32,
  },
}

/*
fn same_owner(units_a: Option<(PlayerIndex, i32)>, units_b: Option<(PlayerIndex, i32)>) -> bool {
  match (units_a, units_b) {
    (Some((a, _)), Some((b, _))) => a == b,
    _ => false,
  }
}
*/

#[derive(Debug)]
pub struct GameState {
  pub rng:                     Rng,
  pub territories:             Vec<Territory>,
  pub player_states:           Vec<PlayerState>,
  pub player_indices_by_token: HashMap<PlayerToken, PlayerIndex>,
}

impl GameState {
  pub fn new(seed: u64) -> Self {
    Self {
      rng:                     Rng::new_from_seed(seed),
      territories:             vec![],
      player_states:           vec![],
      player_indices_by_token: HashMap::new(),
    }
  }

  pub fn process_action(
    &mut self,
    player_token: &PlayerToken,
    action: GameAction,
  ) -> Result<(), Error> {
    // Get and validate the player.
    let player_index = match self.player_indices_by_token.get(player_token) {
      Some(player_index) => *player_index,
      None => bail!("Player not found"),
    };
    let player = &mut self.player_states[player_index];
    if !player.is_alive {
      bail!("Player already dead");
    }

    match action {
      GameAction::SetCommand { territory, command } => {
        let command_terr = match self.territories.get(territory) {
          Some(command_terr) => command_terr,
          None => bail!("Territory not found"),
        };
        let (owner, _) = command_terr.contents.ok_or_else(|| anyhow!("Territory is empty"))?;
        if owner != player_index {
          bail!("Player does not own territory");
        }
        // Validate the command.
        match command {
          Command::Attack { target } => {
            if let None = self.territories.get(target) {
              bail!("Target territory not found");
            }
            if !command_terr.adjacent.contains(&target) {
              bail!("Target territory not adjacent");
            }
          }
          Command::Fortify | Command::Grow => {}
        }
        // Set the command.
        self.territories[territory].command = command;
      }
      GameAction::Resign => player.is_alive = false,
    }

    Ok(())
  }

  //pub fn sample_win_rate(&mut self, half_atk: i32, half_def: i32) -> bool {
  //
  //}

  pub fn step_time(&mut self) {
    // Each territory's defense points are:
    // - The number of units in the territory, or half if it's attacking.
    // - An adjustment for the territory sort (-1 for swamp, +1 for forest).
    // - An adjustment for fortification (+1).
    // - Any friendly units "attacking" the territory sum their units to the defense points.
    // - The owner's defense level is added to the defense points.
    let mut half_defense_points: Vec<i32> = self
      .territories
      .iter()
      .map(|terr| {
        let (owner, units) = match terr.contents {
          Some(pair) => pair,
          _ => return 0,
        };
        2 * self.player_states[owner].defense_level + match terr.command {
          Command::Attack { .. } => units,
          _ => 2 * units,
        } + match terr.sort {
          TerritorySort::Swamp => -2,
          TerritorySort::Forest => 2,
          _ => units,
        } + match terr.command {
          Command::Fortify => 2,
          _ => 0,
        }
      })
      .collect();
    // Each territory's incoming attack points is simply the sum of the units attacking it,
    // plus the attacker's attack level for each attacking territory.
    let mut incoming_half_attack_points: Vec<i32> = vec![0; self.territories.len()];
    for terr in &self.territories {
      let (owner, units) = match terr.contents {
        Some(pair) => pair,
        None => continue,
      };
      if let Command::Attack { target } = terr.command {
        // Check who owns the target territory.
        if self.territories[target].contents.map(|(target_owner, _)| target_owner == owner).unwrap_or(false) {
          half_defense_points[target] += units;
        } else {
          incoming_half_attack_points[target] += units;
        }
      }
    }
    let mut animation_events = vec![];
    // Have all dying territories lose their units.
    for (i, terr) in self.territories.iter_mut().enumerate() {
      let mut defense_sum = 0;
      for _ in 0..half_defense_points[i] {
        defense_sum += self.rng.generate() & 0x3;
      }
      let mut attack_sum = 0;
      for _ in 0..incoming_half_attack_points[i] {
        attack_sum += self.rng.generate() & 0x3;
      }

      if attack_sum > defense_sum {
        terr.contents = None;
        animation_events.push(AnimationEvent::Death {
          render_info: terr.render_info,
          // FIXME: Shouldn't be 0.
          amount:      0, //terr.units,
        });
      }
    }
    // For each territory, move a random territory among all that want to move in with the most units into it.
    #[derive(Clone, Copy)]
    struct IncomingEntry {
      units:            i32,
      source_territory: Option<TerritoryIndex>,
      competitor_count: u64,
    }
    let mut best_incoming = vec![
      IncomingEntry {
        units:            -1,
        source_territory: None,
        competitor_count: 0,
      };
      self.territories.len()
    ];
    // We now process each territory, updating where it wants to go.
    for (i, terr) in self.territories.iter().enumerate() {
      let units = match terr.contents {
        Some((_, units)) => units,
        None => continue,
      };
      if let Command::Attack { target } = terr.command {
        // You can only move into empty territories.
        if self.territories[target].contents.is_some() {
          continue;
        }
        // Check if this is a new best.
        let is_new_best = match units.cmp(&best_incoming[target].units) {
          Ordering::Greater => {
            best_incoming[target].competitor_count = 1;
            true
          }
          Ordering::Equal => {
            best_incoming[target].competitor_count += 1;
            // If it's a tie, we have a 1/competitor_count chance of being the new best.
            self.rng.generate() % best_incoming[target].competitor_count == 0
          }
          Ordering::Less => false,
        };
        if is_new_best {
          best_incoming[target].units = units;
          best_incoming[target].source_territory = Some(i);
        }
      }
    }
    // Now we actually move the units.
    for (target_terr_index, incoming_entry) in best_incoming.iter().enumerate() {
      if let Some(source_terr_index) = incoming_entry.source_territory {
        let contents = self.territories[source_terr_index].contents;
        self.territories[target_terr_index].contents = contents;
        self.territories[source_terr_index].contents = None;
        animation_events.push(AnimationEvent::Movement {
          render_info_from: self.territories[source_terr_index].render_info,
          render_info_to:   self.territories[target_terr_index].render_info,
          // FIXME: Shouldn't be 0.
          amount:      0,
        });
      }
    }
  }
}
