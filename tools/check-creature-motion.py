"""Verify native Bevy joint transforms, forward convention, state changes and clip blending."""
import argparse
import json
import math
from pathlib import Path

STATES = ["IdleGround", "Walk", "Takeoff", "Flap", "Glide", "Land", "IdleGround"]

def verify(data):
    samples = data["creature_motion"]["samples"]
    sequence, contacts, alignment, peak = [], [], [], 0
    last = None
    peak_blend = 0
    actual_scale = []
    for sample in samples:
        actor = next(a for a in sample["actors"] if a["slot"] == 0)
        state = actor["state"]
        if not sequence or state != sequence[-1]:
            sequence.append(state)
        head, body = [actor["joints"][name]["global"] for name in ("Head", "Body")]
        length = math.hypot(head[0]-body[0], head[2]-body[2])
        forward = actor["forward"]
        alignment.append(((head[0]-body[0])*forward[0]+(head[2]-body[2])*forward[2])/length)
        peak_blend=max(peak_blend,sum(clip["weight"]>.001 for clip in actor["clips"]))
        for joint in actor["joints"].values():
            actual_scale.extend(abs(v-1) for v in joint["scale"])
        if last:
            dt=actor["elapsed_s"]-last["elapsed_s"]
            if dt>0:
                speed=math.dist(actor["world"],last["world"])/dt
                peak=max(speed,peak)
                if state in ("Walk","Flap","Glide") and state==last["state"]:
                    dx,dz=actor["world"][0]-last["world"][0],actor["world"][2]-last["world"][2]
                    if math.hypot(dx,dz)>.05:
                        contacts.append((dx*forward[0]+dz*forward[2])/math.hypot(dx,dz))
        last=actor
    assert sequence==STATES,sequence
    assert min(alignment)>.85,(min(alignment),"head facing backwards")
    assert min(contacts)>.8,(min(contacts),"travelling sideways")
    assert peak_blend>=2 and max(actual_scale)<.0001
    assert samples[-1]["actors"][0]["world"][1]-samples[0]["actors"][0]["world"][1]<.1
    top=max(s["actors"][0]["world"][1] for s in samples)
    assert top-samples[0]["actors"][0]["world"][1]>30 and peak<50
    return dict(states=sequence,peak_speed_blocks_s=peak,peak_altitude=top,
                minimum_head_forward_dot=min(alignment),minimum_movement_forward_dot=min(contacts),
                maximum_joint_scale_error=max(actual_scale),maximum_live_blended_clips=peak_blend,
                camera= data["final_eye"],sample_count=len(samples),speed=samples[-1]["playback_speed"])

if __name__=="__main__":
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("reports",nargs="+",type=Path)
    p.add_argument("--output",type=Path,required=True)
    a=p.parse_args()
    verified=[verify(json.loads(path.read_text(encoding="utf8"))) for path in a.reports]
    a.output.write_text(json.dumps(verified,indent=2),encoding="utf8")
    print(json.dumps(verified))
