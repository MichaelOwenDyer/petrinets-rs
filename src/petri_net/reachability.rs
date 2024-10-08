//! This module performs reachability analysis on a Petri net

use super::{Arc, CapacityFn, PetriNet, PlaceId, TransitionId, WeightFn};
use derive_more::Display as DeriveDisplay;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::hash::Hash;

/// A number of tokens in a place
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, DeriveDisplay)]
pub struct Tokens(pub usize);

/// A unique ID for a marking in the reachability graph.
/// Displayed as "M" followed by the ID padded by 3 leading 0s, e.g. M000, M001, M002, ...
#[derive(Debug, Clone, Copy, DeriveDisplay)]
#[display(fmt = "M{:03}", _0)]
pub struct MarkingId(usize);

/// A marking function is a mapping from place IDs to the number of tokens in each place
/// It is used to keep track of the current state of the Petri net
pub trait MarkingFn: Clone + Eq + Hash {
    /// Get the marking at a place
    fn get(&self, id: &PlaceId) -> Tokens;
    /// Set the marking at a place
    fn set(&mut self, id: PlaceId, tokens: Tokens);
}

/// A marking function which is implemented as a BTreeMap (due to its consistent ordering and hashing properties)
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub struct Marking(BTreeMap<PlaceId, Tokens>);

/// TODO: Implement function to see if a marking M is coverable in (P, M0)
impl Marking {
    /// Returns true if this marking is covered by another marking.
    /// A marking is covered by another marking if the other marking has at least as many tokens on each place.
    /// This can be used to detect unbounded places.
    pub fn covered_by(&self, other: &Self) -> bool {
        self.0
            .iter()
            .all(|(id, own_tokens)| other.get(id).0 >= own_tokens.0)
    }
}

impl MarkingFn for Marking {
    fn get(&self, id: &PlaceId) -> Tokens {
        // If the place is not in the marking, we assume it has 0 tokens
        self.0.get(id).copied().unwrap_or_default()
    }
    fn set(&mut self, id: PlaceId, tokens: Tokens) {
        // Internal implementation detail:
        // We only store places with non-zero tokens in the BTreeMap
        if tokens.0 == 0 {
            self.0.remove(&id);
        } else {
            self.0.insert(id, tokens);
        }
    }
}

impl<P: Into<PlaceId>, T: Into<Tokens>> FromIterator<(P, T)> for Marking {
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = (P, T)>,
    {
        let mut marking = Marking::default();
        for (id, tokens) in iter {
            marking.set(id.into(), tokens.into());
        }
        marking
    }
}

/// A continuation is a transition that can be fired from a marking, resulting in a new marking.
/// If the resulting marking has been seen before, the continuation might be a loop.
/// Displayed as "{T}->{M}", e.g. T0->M000, T1->M001, ...
#[derive(Debug, Clone, Copy, DeriveDisplay)]
#[display(fmt = "{}->{}", _0, _1)]
pub struct Continuation(TransitionId, MarkingId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, DeriveDisplay)]
pub enum Bound {
    #[display(fmt = "{}-Bounded", _0)]
    Bounded(Tokens),
    #[display(fmt = "Unbounded")]
    #[expect(unused)] // Will be unused until unboundedness checking is implemented
    Unbounded,
}

impl PartialOrd for Bound {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Bound {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Bound::Bounded(a), Bound::Bounded(b)) => a.cmp(b),
            (Bound::Unbounded, Bound::Unbounded) => Ordering::Equal,
            (Bound::Bounded(_), Bound::Unbounded) => Ordering::Greater,
            (Bound::Unbounded, Bound::Bounded(_)) => Ordering::Less,
        }
    }
}

/// Describes the maximum number of tokens stored on a place at any point in time
#[derive(Debug, Clone)]
pub struct Boundedness(Vec<Bound>);

impl Boundedness {
    /// Creates a new Boundedness object with all places in the net set to 0
    fn new<C: CapacityFn, W: WeightFn>(net: &PetriNet<C, W>) -> Self {
        let mut vec = vec![Bound::Bounded(Tokens(0)); net.places.len()];
        // Update the boundedness with the initial marking
        for (place_id, &initial_tokens) in net.initial_marking.0.iter() {
            vec[place_id.0] = Bound::Bounded(initial_tokens);
        }
        Self(vec)
    }
    /// Updates the boundedness of a place if the new value is greater than the old value
    fn update(&mut self, place_id: PlaceId, bound: Bound) {
        self.0[place_id.0] = std::cmp::max(self.0[place_id.0], bound);
    }
}

/// Transition liveness classes describe how many times a transition fires
/// TODO: Copy definitions from paper
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, DeriveDisplay)]
pub enum Live {
    /// Can never fire
    L0,
    /// Fires a finite and deterministic number of times
    L1,
    /// Fires a finite but non-deterministic number of times
    #[expect(unused)]
    L2,
    /// Fires a non-deterministically finite or infinite number of times
    #[expect(unused)]
    L3,
    /// Fires a deterministically infinite number of times
    L4,
}

/// Liveness is a list of liveness classes for each transition in the Petri net (ID = index)
#[derive(Debug, Clone)]
pub struct Liveness(Vec<Live>);

impl Liveness {
    /// Create a new liveness map from a list of transitions
    fn new<C: CapacityFn, W: WeightFn>(net: &PetriNet<C, W>) -> Self {
        Self(vec![Live::L0; net.transitions.len()])
    }
    /// Updates the liveness of a transition if the new value is greater than the old value
    fn update(&mut self, transition_id: TransitionId, live: Live) {
        self.0[transition_id.0] = std::cmp::max(self.0[transition_id.0], live);
    }
}

/// A helper struct for displaying a list of items separated by a delimiter
struct Join<'a, T: Display>(&'a [T], &'a str);

impl<'a, T: Display> Display for Join<'a, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        let mut iter = self.0.iter();
        if let Some(first) = iter.next() {
            write!(f, "{}", first)?;
        }
        for item in iter {
            write!(f, "{}{}", self.1, item)?;
        }
        Ok(())
    }
}

/// Liveness is displayed in the format L0(T1, T2); L1(T3, T4); ...
impl Display for Liveness {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        struct LivenessClass(&'static str, Vec<TransitionId>);
        impl Display for LivenessClass {
            fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
                write!(f, "{} ({})", self.0, Join(&self.1, ", "))
            }
        }
        // TODO: Can we avoid allocating here?
        let mut l = [
            LivenessClass("L0", vec![]),
            LivenessClass("L1", vec![]),
            LivenessClass("L2", vec![]),
            LivenessClass("L3", vec![]),
            LivenessClass("L4", vec![]),
        ];
        for (i, &live) in self.0.iter().enumerate() {
            l[live as usize].1.push(TransitionId(i));
        }
        write!(f, "{}", Join(&l, "; "))
    }
}

/// A transition ID and the IDs of its input places and of its output places
/// This allows for easy checking of whether a transition can fire from a given marking
#[derive(Debug, Clone)]
struct TransitionIO {
    id: TransitionId,
    inputs: Vec<PlaceId>,
    outputs: Vec<PlaceId>,
}

/// Struct for keeping track of the markings we have seen before and their IDs
/// TODO: Change out the HashMap for a tree-like data structure for tracking paths
#[derive(Debug, Default)]
struct Markings {
    markings: HashMap<Marking, MarkingId, ahash::RandomState>,
}

impl Markings {
    /// Insert a new marking into the map and return its ID
    fn remember(&mut self, marking: Marking) -> MarkingId {
        let id = MarkingId(self.markings.len());
        self.markings.insert(marking, id);
        id
    }
    /// Get the ID of a marking, if it exists
    fn look_up(&self, marking: &Marking) -> Option<MarkingId> {
        self.markings.get(marking).copied()
    }
}

#[derive(Debug, Clone)]
#[expect(unused)]
pub struct IncidenceMatrix<'net, C: CapacityFn, W: WeightFn> {
    petri_net: &'net PetriNet<C, W>,
    matrix: Vec<Vec<isize>>,
}

/// A reachability graph is a list of markings, each with a unique ID,
/// and each with a list of the transitions that can be fired from them and the IDs of the resulting markings
#[derive(Debug, Clone)]
pub struct ReachabilityAnalysis<'net, C: CapacityFn, W: WeightFn> {
    petri_net: &'net PetriNet<C, W>,
    pub rows: Vec<(MarkingId, Marking, Vec<Continuation>)>,
    pub boundedness: Boundedness,
    pub liveness: Liveness,
}

impl<C: CapacityFn, W: WeightFn> PetriNet<C, W> {
    /// Create a Vec<TransitionIO> for efficient transition firing.
    /// The indices of the transitions in this vector correspond to the indices of the transitions in the Petri net, and their IDs
    fn transition_io(&self) -> Vec<TransitionIO> {
        let mut transitions = Vec::with_capacity(self.transitions.len());
        // For each transition in the net, collect its ID, input places, and output places
        for transition in &self.transitions {
            let id = transition.id;
            let mut inputs = Vec::new();
            let mut outputs = Vec::new();
            for arc in &self.arcs {
                match *arc {
                    Arc::PlaceTransition(source, target) if target == transition.id => {
                        inputs.push(source);
                    }
                    Arc::TransitionPlace(source, target) if source == transition.id => {
                        outputs.push(target);
                    }
                    _ => {}
                }
            }
            transitions.push(TransitionIO { id, inputs, outputs });
        }
        transitions
    }
    /// Compute the incidence matrix for detecting unboundedness
    #[expect(unused)]
    fn incidence_matrix(&self, transition_io: &[TransitionIO]) -> IncidenceMatrix<'_, C, W> {
        let mut matrix: Vec<Vec<isize>> = vec![vec![0; self.transitions.len()]; self.places.len()];
        for (j, transition) in transition_io.iter().enumerate() {
            for &input in &transition.inputs {
                if let Some(i) = self.places.iter().position(|place| place.id == input) {
                    matrix[i][j] -= self.weights
                        .get_or_default(&Arc::PlaceTransition(input, transition.id))
                        .0 as isize;
                }
            }
            for &output in &transition.outputs {
                if let Some(i) = self.places.iter().position(|place| place.id == output) {
                    matrix[i][j] += self.weights
                        .get_or_default(&Arc::TransitionPlace(transition.id, output))
                        .0 as isize;
                }
            }
        }
        IncidenceMatrix {
            petri_net: self,
            matrix,
        }
    }
    /// Fires all enabled transitions in the Petri net from the provided marking,
    /// and returns a list of the resulting markings.
    /// This attempts to fire all transitions, but silently fails for those that are not enabled.
    /// This function also updates the place boundedness and transition liveness.
    #[rustfmt::skip]
    fn fire_transitions(
        transition_io: &[TransitionIO],
        marking: &Marking,
        capacities: &C,
        weights: &W,
        boundedness: &mut Boundedness,
        liveness: &mut Liveness,
    ) -> Vec<(TransitionId, Marking)> {
        transition_io.iter().filter_map(|transition| {
            // Create a clone of the start marking to modify
            let mut marking = marking.clone();
            // Start by checking that all the input places have sufficient tokens to fire the transition
            transition.inputs.iter().try_for_each(|&source_place| {
                let current_tokens = marking.get(&source_place).0;
                let token_requirement = weights.get_or_default(&Arc::PlaceTransition(source_place, transition.id)).0;
                current_tokens.checked_sub(token_requirement)
                    .map(|new_tokens| marking.set(source_place, Tokens(new_tokens)))
                    .ok_or(()) // Produce Ok if tokens were removed, Err if not enough tokens
            // Then check that all outputs have enough capacity to store the new tokens
            }).and_then(|_| transition.outputs.iter().try_for_each(|&target_place| {
                let current_tokens = marking.get(&target_place).0;
                let output_weight = weights.get_or_default(&Arc::TransitionPlace(transition.id, target_place)).0;
                let capacity = capacities.get_or_default(&target_place).0;
                capacity.checked_sub(output_weight)
                    .filter(|&max_current_tokens| current_tokens <= max_current_tokens)
                    .map(|_| {
                        let new_tokens = Tokens(current_tokens + output_weight);
                        // If so, add the tokens to the target place
                        marking.set(target_place, new_tokens);
                        // Since we are increasing tokens on a place, we need to update the boundedness
                        boundedness.update(target_place, Bound::Bounded(new_tokens));
                    })
                    .ok_or(()) // Produce Ok if tokens were added, Err if not enough capacity
            // If the transition fired successfully, return its ID and the resulting marking
            }))
                .ok()
                .map(|_| {
                    // This transition fired successfully, so it must be at least L1-live
                    liveness.update(transition.id, Live::L1);
                    (transition.id, marking)
                })
        }).collect() // Collect all successful firing attempts
    }
    /// Perform a reachability analysis on the Petri net
    pub fn reachability_analysis(&self) -> ReachabilityAnalysis<'_, C, W> {
        let mut analysis = ReachabilityAnalysis::new(self);
        let mut markings = Markings::default();
        let id = markings.remember(self.initial_marking.clone());
        let transition_io = self.transition_io();
        let mut queue = VecDeque::new();
        // Start the reachability analysis with the initial marking and its enabled transitions
        queue.push_back((
            id,
            self.initial_marking.clone(),
            PetriNet::fire_transitions(
                &transition_io,
                &self.initial_marking,
                &self.capacities,
                &self.weights,
                &mut analysis.boundedness,
                &mut analysis.liveness,
            ),
        ));
        while let Some((source_marking_id, source_marking, branches_to_explore)) = queue.pop_front() {
            let mut continuations = Vec::with_capacity(branches_to_explore.len());
            for (transition_id, resulting_marking) in branches_to_explore {
                if let Some(existing_marking_id) = markings.look_up(&resulting_marking) {
                    // TODO: Fix loop detection (find path from marking to itself)
                    // TODO: Detect L3/L4 transitions
                    // If we have seen this marking before, don't explore it again
                    continuations.push(Continuation(transition_id, existing_marking_id));
                } else {
                    // If we have not seen this marking before, remember it and explore it
                    let new_marking_id = markings.remember(resulting_marking.clone());
                    continuations.push(Continuation(transition_id, new_marking_id));
                    // Fire all enabled transitions from the new marking
                    let new_branches = PetriNet::fire_transitions(
                        &transition_io,
                        &resulting_marking,
                        &self.capacities,
                        &self.weights,
                        &mut analysis.boundedness,
                        &mut analysis.liveness,
                    );
                    queue.push_back((new_marking_id, resulting_marking, new_branches));
                }
            }
            analysis.rows.push((source_marking_id, source_marking, continuations));
        }
        analysis
    }
}

/// How to interpret a deadlock in the reachability graph
/// A final (desired) deadlock is a marking with only one token on a place with no outgoing arcs
/// Any other deadlock is a non-final (undesired) deadlock
#[derive(Debug, Clone, DeriveDisplay)]
pub enum DeadlockInterpretation {
    #[display(fmt = "final")]
    Final,
    #[display(fmt = "deadlock")]
    Deadlock,
}

impl<'net, C: CapacityFn, W: WeightFn> ReachabilityAnalysis<'net, C, W> {
    /// Create a new reachability analysis for the given Petri net
    fn new(petri_net: &'net PetriNet<C, W>) -> Self {
        Self {
            petri_net,
            rows: Vec::new(),
            boundedness: Boundedness::new(petri_net),
            liveness: Liveness::new(petri_net),
        }
    }
    /// Returns a list of deadlocked markings and their interpretation
    #[rustfmt::skip]
    fn deadlocks(&self) -> Vec<(MarkingId, DeadlockInterpretation)> {
        self.rows.iter().filter_map(|(marking_id, marking, continuations)| {
            if !continuations.is_empty() {
                return None; // Not a deadlock because there exists a continuation out of this marking
            }
            // Interpret the deadlock
            let interpretation = {
                // Find all places with tokens
                let places_with_tokens: Vec<(&PlaceId, &Tokens)> = marking.0.iter()
                    .filter(|(_, &tokens)| tokens.0 > 0)
                    .collect();
                match places_with_tokens.as_slice() {
                    // A final deadlock marking must contain only one place with one token
                    &[(place_id, Tokens(1))] if !self.petri_net.arcs.iter().any(|arc| {
                        // and there must be no outgoing arcs from that place
                        matches!(arc, Arc::PlaceTransition(source, _) if source == place_id)
                    }) => DeadlockInterpretation::Final,
                    // Otherwise, we have a regular deadlock
                    _ => DeadlockInterpretation::Deadlock,
                }
            };
            Some((*marking_id, interpretation))
        }).collect()
    }
    /// Returns the maximum boundedness of any place in the Petri net
    #[rustfmt::skip]
    fn boundedness(&self) -> Bound {
        self.boundedness.0.iter().copied().max().unwrap_or(Bound::Bounded(Tokens(0)))
    }
    /// Returns true if every place in the Petri net is 1-bounded
    #[rustfmt::skip]
    fn is_safe(&self) -> bool {
        self.boundedness.0.iter().all(|&bound| bound == Bound::Bounded(Tokens(1)))
    }
    /// Returns true if every transition in the Petri net is L4-live
    fn is_live(&self) -> bool {
        self.liveness.0.iter().all(|&live| live == Live::L4)
    }
    /// Returns true if at least one transition in the Petri net is L4-live
    /// and at least one transition is not L4-live
    fn is_quasi_live(&self) -> bool {
        !self.is_live() && self.liveness.0.iter().any(|&live| live == Live::L4)
    }
    /// Returns the markings from which we can reach a previous marking,
    /// forming a loop in the reachability graph
    fn loops(&self) -> Vec<MarkingId> {
        vec![] // TODO: Implement loop detection
    }
    /// Returns true if all places had at least one token at some point,
    /// and all transitions fired at least once
    #[rustfmt::skip]
    fn is_sound(&self) -> bool {
        self.liveness.0.iter().all(|&live| live != Live::L0)
            && self.boundedness.0.iter().all(|&bound| bound > Bound::Bounded(Tokens(0)))
    }
}

/// Display a reachability analysis as a table
impl<'net, C: CapacityFn, W: WeightFn> Display for ReachabilityAnalysis<'net, C, W> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        // Print all transitions and their names
        for transition in &self.petri_net.transitions {
            writeln!(f, "{} ... {}", transition.id, transition.name)?;
        }
        writeln!(f)?;

        // Print the top row of the reachability graph, starting with "M"
        write!(f, "{:<7}", "M")?;
        // ...and then all place IDs
        for place in &self.petri_net.places {
            write!(f, "{:<5}", place.id.to_string())?;
        }
        // ...and then "Transitions"
        writeln!(f, "Transitions")?;

        // Print the body of the reachability graph
        for (marking_id, marking, continuations) in &self.rows {
            // Print the ID of this row's marking
            write!(f, "{:<7}", marking_id.to_string())?;
            // For each place, print the number of tokens on that place in this marking
            for place in &self.petri_net.places {
                write!(f, "{:<5}", marking.get(&place.id).0)?;
            }
            // Print the transitions which can fire from this marking and the markings they lead to
            writeln!(f, "{}", Join(continuations, ", "))?;
        }
        writeln!(f)?;

        writeln!(f, "Interpretation")?;
        for (marking_id, interpretation) in self.deadlocks() {
            writeln!(f, "{}: {}", marking_id, interpretation)?;
        }
        writeln!(f, "Boundedness: {}", self.boundedness())?;
        writeln!(f, "Safe: {}", self.is_safe())?;
        writeln!(f, "Live: {}", self.is_live())?;
        writeln!(f, "Quasi-Live: {}", self.is_quasi_live())?;
        writeln!(f, "Liveness: {}", self.liveness)?;
        writeln!(f, "Loops: {}", Join(&self.loops(), ", "))?;
        writeln!(f, "Sound: {}", self.is_sound())?;
        Ok(())
    }
}
