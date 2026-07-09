//! A small typed-DSL frontend: a builder that constructs the workflow IR
//! together with its semantic annotations. This is the *greenfield* authoring
//! path — unlike the ASL frontend it carries full type/typestate/idempotency
//! information by construction, so every check applies with maximum precision.
//!
//! `order_example` is the running order-processing saga used as the paper's
//! worked example; the `bad` variant ships before charging, which the typestate
//! (SC2001) and typed-contract (SC1010) checks reject.

use crate::annot::{ProtocolEdge, SchemaDef, Sidecar, TaskAnnot};
use crate::ir::*;
use serde_json::json;

/// A minimal fluent builder over the IR (the DSL surface).
pub struct Builder {
    wf: Workflow,
    sidecar: Sidecar,
}

impl Builder {
    pub fn new(name: &str, start: &str) -> Self {
        Builder {
            wf: Workflow::new(name, start),
            sidecar: Sidecar::default(),
        }
    }

    /// Declare a named record schema (set of fields).
    pub fn schema(mut self, name: &str, fields: &[&str]) -> Self {
        self.sidecar.schemas.insert(
            name.to_string(),
            SchemaDef { fields: fields.iter().map(|s| s.to_string()).collect() },
        );
        self
    }

    /// Declare an allowed business-state transition.
    pub fn protocol(mut self, from: &str, to: &str) -> Self {
        self.sidecar
            .protocol
            .push(ProtocolEdge { from: from.into(), to: to.into() });
        self
    }

    /// Add a typed task. `next`/`catch` wire control flow; the rest are the
    /// task's declared semantics that the verifier consumes.
    #[allow(clippy::too_many_arguments)]
    pub fn task(
        mut self,
        name: &str,
        state_in: &str,
        state_out: &str,
        idempotent: bool,
        persistent: bool,
        compensation: Option<&str>,
        next: Option<&str>,
        catch: Option<&str>,
    ) -> Self {
        let mut st = State::new(name, StateKind::Task);
        st.resource = Some("arn:aws:states:::lambda:invoke".into());
        st.parameters = Some(json!({
            "FunctionName": format!("${{{name}Function}}"),
            "Payload.$": "$"
        }));
        if let Some(n) = next {
            st.next = Some(n.into());
        } else {
            st.end = true;
        }
        if let Some(c) = catch {
            st.catch.push(CatchRule {
                error_equals: vec!["States.ALL".into()],
                next: c.into(),
                result_path: ResultPath::Path("$.error".into()),
                assign: None,
                extra: serde_json::Map::new(),
            });
        }
        self.wf.states.insert(name.to_string(), st);
        self.sidecar.tasks.insert(
            name.to_string(),
            TaskAnnot {
                idempotent: Some(idempotent),
                persistent: Some(persistent),
                effect: None,
                compensation: compensation.map(String::from),
                state_in: Some(state_in.into()),
                state_out: Some(state_out.into()),
                input_schema: Some(state_in.into()),
                output_schema: Some(state_out.into()),
            },
        );
        self
    }

    /// Add a plain (untyped) task used as a compensator, terminating at `next`.
    pub fn compensator(mut self, name: &str, next: &str) -> Self {
        let mut st = State::new(name, StateKind::Task);
        st.resource = Some("arn:aws:states:::lambda:invoke".into());
        st.parameters = Some(json!({ "FunctionName": format!("${{{name}Function}}"), "Payload.$": "$" }));
        st.next = Some(next.into());
        self.wf.states.insert(name.to_string(), st);
        self
    }

    pub fn succeed(mut self, name: &str) -> Self {
        let mut st = State::new(name, StateKind::Succeed);
        st.end = true;
        self.wf.states.insert(name.to_string(), st);
        self
    }

    pub fn fail(mut self, name: &str) -> Self {
        let mut st = State::new(name, StateKind::Fail);
        st.end = true;
        self.wf.states.insert(name.to_string(), st);
        self
    }

    pub fn build(self) -> (Workflow, Sidecar) {
        (self.wf, self.sidecar)
    }
}

/// The order-processing saga. `bad == true` ships before charging.
pub fn order_example(bad: bool) -> (Workflow, Sidecar) {
    // forward control flow differs between the valid and reordered variants
    let (after_reserve, after_charge, after_ship) = if bad {
        // Create -> Reserve -> Ship -> Charge -> Done
        ("ShipOrder", "OrderCompleted", "ChargeCard")
    } else {
        // Create -> Reserve -> Charge -> Ship -> Done
        ("ChargeCard", "ShipOrder", "OrderCompleted")
    };

    // Reverse-order compensation chain (a correct Saga): the compensator of the
    // last committed step runs first and hands off to the previous step's
    // compensator, so a failure at any step unwinds *all* prior effects. The chain
    // follows the actual forward order, so both the valid and reordered variants
    // are Saga-complete (SC4011) — only the reordering's typestate/contract faults
    // remain. `comps` lists the compensators in forward-task order.
    let comps: [&str; 4] = if bad {
        ["CancelOrder", "ReleaseInventory", "CancelShipment", "RefundPayment"]
    } else {
        ["CancelOrder", "ReleaseInventory", "RefundPayment", "CancelShipment"]
    };

    let b = Builder::new("order-processing", "CreateOrder")
        .schema("CustomerRequest", &["customerId", "items"])
        .schema("OrderCreated", &["orderId", "amount", "items"])
        .schema("OrderReserved", &["orderId", "amount", "reservationId"])
        .schema("OrderPaid", &["orderId", "paymentId", "reservationId"])
        .schema("OrderShipped", &["orderId", "trackingId"])
        .protocol("CustomerRequest", "OrderCreated")
        .protocol("OrderCreated", "OrderReserved")
        .protocol("OrderReserved", "OrderPaid")
        .protocol("OrderPaid", "OrderShipped")
        // forward tasks
        .task("CreateOrder", "CustomerRequest", "OrderCreated", false, true, Some("CancelOrder"), Some("ReserveInventory"), Some("CancelOrder"))
        .task("ReserveInventory", "OrderCreated", "OrderReserved", false, true, Some("ReleaseInventory"), Some(after_reserve), Some("ReleaseInventory"))
        .task("ChargeCard", "OrderReserved", "OrderPaid", false, true, Some("RefundPayment"), Some(after_charge), Some("RefundPayment"))
        .task("ShipOrder", "OrderPaid", "OrderShipped", false, true, Some("CancelShipment"), Some(after_ship), Some("CancelShipment"));
    // compensators wired as a reverse-order chain ending at the Fail terminal
    let b = chain_compensators(b, &comps, "OrderFailed");
    b.succeed("OrderCompleted").fail("OrderFailed").build()
}

/// Wire a list of compensators (given in forward-task order) into a reverse-order
/// compensation chain: the last one runs first and each hands off to the previous
/// step's compensator, with the earliest compensator ending at `fail`.
fn chain_compensators(mut b: Builder, comps: &[&str], fail: &str) -> Builder {
    for i in 0..comps.len() {
        let next = if i == 0 { fail } else { comps[i - 1] };
        b = b.compensator(comps[i], next);
    }
    b
}

/// A second, unrelated workflow authored entirely in the DSL: a travel-booking
/// saga (reserve flight, reserve hotel, charge traveler). `bad == true` charges
/// the traveler before the hotel is reserved, which the typestate (SC2001) and
/// typed-contract (SC1010) checks reject. Demonstrates that the DSL is a general
/// authoring surface, not a single hard-coded example.
pub fn travel_example(bad: bool) -> (Workflow, Sidecar) {
    let (after_flight, after_hotel, after_charge) = if bad {
        // Flight -> Charge -> Hotel -> Done  (charge before the hotel exists)
        ("ChargeTraveler", "TravelDone", "ReserveHotel")
    } else {
        // Flight -> Hotel -> Charge -> Done
        ("ReserveHotel", "ChargeTraveler", "TravelDone")
    };

    // reverse-order compensation chain in forward-task order (see order_example)
    let comps: [&str; 3] = if bad {
        ["CancelFlight", "RefundTraveler", "CancelHotel"]
    } else {
        ["CancelFlight", "CancelHotel", "RefundTraveler"]
    };

    let b = Builder::new("travel-booking", "ReserveFlight")
        .schema("TravelRequest", &["customerId", "dates"])
        .schema("FlightReserved", &["bookingId", "flightId"])
        .schema("HotelReserved", &["bookingId", "flightId", "hotelId"])
        .schema("TravelPaid", &["bookingId", "paymentId"])
        .protocol("TravelRequest", "FlightReserved")
        .protocol("FlightReserved", "HotelReserved")
        .protocol("HotelReserved", "TravelPaid")
        // forward tasks
        .task("ReserveFlight", "TravelRequest", "FlightReserved", false, true, Some("CancelFlight"), Some(after_flight), Some("CancelFlight"))
        .task("ReserveHotel", "FlightReserved", "HotelReserved", false, true, Some("CancelHotel"), Some(after_hotel), Some("CancelHotel"))
        .task("ChargeTraveler", "HotelReserved", "TravelPaid", false, true, Some("RefundTraveler"), Some(after_charge), Some("RefundTraveler"));
    let b = chain_compensators(b, &comps, "TravelFailed");
    b.succeed("TravelDone").fail("TravelFailed").build()
}
