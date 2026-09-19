use backhopper_core::model::verdict::TargetAxisSlot;
use backhopper_core::model::apply::ApplyForecast;

fn set_apply(slot: &mut TargetAxisSlot, forecast: ApplyForecast) {
    slot.apply = Some(forecast);
}

fn main() {}
