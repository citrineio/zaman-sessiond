import QtQuick
import Quickshell.Services.Pipewire

// Read-only producer loaded by the canonical notification renderer.
Item {
    id:events
    property var client
    readonly property PwNode sink: Pipewire.defaultAudioSink
    readonly property bool ready: Pipewire.ready && sink !== null && sink.ready && sink.audio !== null && sink.audio.channels.length > 0
    readonly property string signature: ready ? JSON.stringify([sink.name, sink.description || sink.nickname || sink.name, sink.audio.muted]) : ""
    property var previous: null
    PwObjectTracker { objects: events.sink ? [events.sink] : [] }
    onSignatureChanged: settle.restart()
    Timer {
        id:settle
        interval:500
        onTriggered: {
            if (!events.ready) { events.previous = null; return; }
            var now = JSON.parse(events.signature);
            var before = events.previous;
            events.previous = now;
            if (before === null || !events.client || !events.client.available) return;
            if (before[0] !== now[0]) events.client.send({action:"event", source:"Zaman", key:"audio:output:" + now[0], title:"Audio output changed", body:String(now[1]).slice(0,100), category:"audio", state:"ready"});
            else if (before[2] !== now[2]) events.client.send({action:"event", source:"Zaman", key:"audio:mute:" + now[2], title:now[2] ? "Sound muted" : "Sound on", category:"audio", state:now[2] ? "muted" : "ready"});
            // Volume changes deliberately do not enter persistent history.
        }
    }
}
