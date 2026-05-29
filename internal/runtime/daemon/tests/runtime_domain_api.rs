use lilo_rm_core::{
    CaptureRequest, CaptureResponse, DoctorResponse, EventBatch, EventsRequest, KillByPidRequest,
    KillByPidResponse, KillOutcome, KillRequest, Lifecycle, NudgeRequest, NudgeResponse,
    RuntimeEvent, SpawnRequest, StatusFilter,
};
use lilo_runtime_daemon::{RuntimeService, SpawnOutcome};
use std::result::Result;

struct RuntimeDomainInputs {
    spawn_req: SpawnRequest,
    events_req: EventsRequest,
    status_filter: StatusFilter,
    kill_req: KillRequest,
    kill_pid_req: KillByPidRequest,
    nudge_req: NudgeRequest,
    capture_req: CaptureRequest,
    event: RuntimeEvent,
}

async fn name_runtime_domain_surface(rs: &RuntimeService, inputs: RuntimeDomainInputs) {
    let RuntimeDomainInputs {
        spawn_req,
        events_req,
        status_filter,
        kill_req,
        kill_pid_req,
        nudge_req,
        capture_req,
        event,
    } = inputs;

    let _: Result<SpawnOutcome, _> = rs.spawn(spawn_req).await;
    let _: EventBatch = rs.poll_events(events_req).await;
    let _: Vec<Lifecycle> = rs.status(status_filter).await;
    let _: Result<KillOutcome, _> = rs.kill_runtime(kill_req).await;
    let _: Result<KillByPidResponse, _> = rs.kill_by_pid(kill_pid_req).await;
    let _: Result<NudgeResponse, _> = rs.nudge_runtime(nudge_req).await;
    let _: Result<CaptureResponse, _> = rs.capture(capture_req).await;
    let _: Result<DoctorResponse, _> = rs.doctor().await;
    let _: Result<RuntimeEvent, _> = rs.append_event(event).await;
    let _: () = rs.drain_shims();
}

#[test]
fn runtime_domain_surface_is_nameable_from_external_crate() {
    let _ = name_runtime_domain_surface;
}
