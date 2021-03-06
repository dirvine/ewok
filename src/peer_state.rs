use name::Name;
use std::collections::BTreeMap;
use std::mem;
use params::NodeParams;

use self::PeerState::*;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum PeerState {
    /// Appeared in all current blocks at some point.
    Confirmed,
    /// Appeared in at least one current block at some point.
    PartiallyConfirmed {
        // Step that the node first appeared in a current block.
        since: u64
    },
    /// Waiting to join, has not yet been included in a current block.
    Unconfirmed {
        since: u64,
    },
    /// Currently disconnected from us.
    Disconnected {
        since: u64,
        previous_state: Box<PeerState>
    }
}

pub struct PeerStates {
    /// States of known peers.
    states: BTreeMap<Name, PeerState>,
    /// Parameters like timeouts, etc.
    params: NodeParams
}

impl PeerStates {
    pub fn new(params: NodeParams) -> Self {
        PeerStates {
            states: BTreeMap::new(),
            params
        }
    }

    /// Names of all known peers.
    pub fn all_peers(&self) -> Vec<Name> {
        self.states.keys().cloned().collect()
    }

    /// Called when we see a NodeJoined message.
    pub fn node_joined(&mut self, name: Name, step: u64) {
        self.states.entry(name).or_insert(Unconfirmed { since: step });
    }

    /// Called when a node becomes current in a single block, but is not valid in all current
    /// blocks.
    pub fn current_in_some(&mut self, name: Name, step: u64) {
        let state = self.states.entry(name).or_insert(PartiallyConfirmed { since: step });

        // If this node was previously unconfirmed, mark it confirmed.
        if let Unconfirmed { .. } = *state {
            *state = PartiallyConfirmed { since: step };
        }
    }

    /// Called when a node becomes current in all current blocks.
    pub fn current_in_all(&mut self, name: Name, _step: u64) {
        let state = self.states.entry(name).or_insert(Confirmed);

        match *state {
            PartiallyConfirmed { .. } | Unconfirmed { .. } => {
                *state = Confirmed;
            }
            _ => ()
        }
    }

    /// Update a node's state in light of a disconnection.
    pub fn disconnected(&mut self, name: Name, step: u64) {
        let state = match self.states.get_mut(&name) {
            Some(s) => s,
            None => {
                println!("warning: out-of-order disconnect, fix that bug!");
                return;
            }
        };

        match *state {
            // Already disconnected, do nothing.
            Disconnected { .. } => {
                println!("warning: out-of-order disconnect");
            }
            // Anything else, update state to disconnected.
            _ => {
                *state = Disconnected {
                    since: step,
                    previous_state: Box::new(state.clone())
                };
            }
        };
    }

    /// Update a node's state in light of a reconnection.
    pub fn reconnected(&mut self, name: Name, _step: u64) {
        let state_ptr = match self.states.get_mut(&name) {
            Some(s) => s,
            None => {
                println!("warning: reconnect before connect");
                return;
            }
        };

        // FIXME: WARNING: nasty borrowck-appeasing hacks.
        let state = mem::replace(state_ptr, Confirmed);

        if let Disconnected { previous_state, .. } = state {
            *state_ptr = *previous_state;
        } else {
            println!("warning: out-of-order reconnect");
            *state_ptr = state;
        }
    }

    /// Return all unconfirmed or partially confirmed nodes who we should keep trying to add.
    /// TODO: implement "nodes that already appear in a current section and are connected to us,
    /// and that at least once in the past 60 seconds have not been missing from any current
    /// section."
    pub fn nodes_to_add(&self, step: u64) -> Vec<Name> {
        self.states.iter().filter(|&(_, state)| {
            match *state {
                PartiallyConfirmed { since } | Unconfirmed { since } => {
                    since >= step.saturating_sub(self.params.join_stabilisation_timeout)
                }
                _ => false
            }
        }).map(|(name, _)| {
            *name
        }).collect()
    }

    /// Return all nodes that we should vote to remove because we are disconnected from them
    /// or are not yet part of all current sections.
    pub fn nodes_to_drop(&self, step: u64) -> Vec<Name> {
        self.states.iter().filter(|&(_, state)| {
            match *state {
                // Remove rule, part 1.
                Disconnected { .. } => {
                    // TODO: use a timeout here?
                    true
                }
                // Remove rule, part 2.
                PartiallyConfirmed { since } | Unconfirmed { since } => {
                    since < step.saturating_sub(self.params.join_stabilisation_timeout)
                }
                _ => false
            }
        }).map(|(name, _)| {
            *name
        }).collect()
    }
}
