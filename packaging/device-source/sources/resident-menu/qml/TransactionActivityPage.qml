import QtQuick

Item {
    id: page
    property var jobs: []
    property bool available: false
    property real uiScale: 1
    property string selectedId: ""
    signal openTransaction(string jobId)
    function px(n) { return Math.round(n * uiScale); }
    function move(direction) {
        if (!jobs.length) return;
        var index = jobs.findIndex(job => job.id === selectedId);
        selectedId = jobs[(index + direction + jobs.length) % jobs.length].id;
        rows.positionViewAtIndex(jobs.findIndex(job => job.id === selectedId), ListView.Contain);
    }
    function activate() { if (available && /^[a-f0-9]{32}$/.test(selectedId)) openTransaction(selectedId); }
    onJobsChanged: if (!jobs.some(job => job.id === selectedId)) selectedId = jobs.length ? jobs[0].id : ""
    ListView {
        id: rows
        anchors.fill: parent
        model: page.jobs; clip: true; spacing: page.px(8)
        delegate: Rectangle {
            required property var modelData
            readonly property bool chosen: modelData.id === page.selectedId
            width: rows.width; height: page.px(103); radius: page.px(12)
            color: chosen ? "#EAC28D" : "#1D2634"
            Column {
                x: page.px(14); y: page.px(12); width: parent.width - page.px(28); spacing: page.px(5)
                MenuText { width: parent.width; text: modelData.name; font.pixelSize: page.px(22); color: chosen ? "#211E1B" : "#ECEEF1"; elide: Text.ElideRight }
                MenuText { width: parent.width; text: (modelData.operation === "install" ? "Install · " : "Remove · ") + modelData.phase + (typeof modelData.percent === "number" ? " · " + modelData.percent + "%" : ""); color: chosen ? "#473627" : "#BBC6D3"; font.pixelSize: page.px(16); elide: Text.ElideRight }
                MenuText { width: parent.width; text: modelData.stage; color: chosen ? "#473627" : "#BBC6D3"; font.pixelSize: page.px(14); elide: Text.ElideRight }
            }
            MouseArea { anchors.fill: parent; onClicked: { page.selectedId = modelData.id; page.activate(); } }
        }
    }
    MenuText { visible: !page.jobs.length; width: parent.width; text: page.available ? "No Dukkan transactions yet." : "Dukkan activity is unavailable. Accepted jobs remain saved."; wrapMode: Text.Wrap; font.pixelSize: page.px(22) }
}
