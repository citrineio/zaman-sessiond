// Exercise the real QML lifecycle and position expression through changing geometry.
const assert = require('node:assert/strict');
const vm = require('node:vm');
const {fixture, source} = require('./menu-controller-regression.cjs');
const position = source.match(/^            x: (offscreenX .*entryProgress)$/m)[1];
const offscreen = source.match(/readonly property real offscreenX: ([^\n]+)/)[1];
let checks = 0;
for (const scale of [0.65, 1, 1.4]) {
  for (const width of [0, 1, 455, 700, 980]) {
    const geometry = {width, root:{px:n=>Math.round(n*scale)}, entryProgress:0};
    vm.createContext(geometry);
    geometry.offscreenX = vm.runInContext(offscreen, geometry);
    assert.ok(vm.runInContext(position, geometry) + width + 5*scale <= 0,
      'panel and shadows must remain entirely outside the left edge at any width');
    geometry.entryProgress = 1;
    assert.equal(vm.runInContext(position, geometry), Math.round(28*scale));
    checks += 2;
  }
}
for (let cycle=0; cycle<50; cycle++) {
  const s = fixture();
  s.menuOpen=false; s.helperIdle=true;
  s.panel.entryProgress=1;
  s.openMenu();
  assert.equal(s.panel.entryProgress,0);
  s.receive(JSON.stringify({type:'ready',foreground:'storefront',storefront:'dukkan',controllers:1}));
  assert.equal(s.entryPhase,'await-frame');
  assert.equal(s.slideIn.starts,0);
  assert.equal(s.writes.filter(x=>x.action==='opened').length,0);
  s.panel.width=0; s.startEntryFrame();
  assert.equal(s.slideIn.starts,0);
  s.panel.width=700; s.overlay.backingWindowVisible=false; s.startEntryFrame();
  assert.equal(s.slideIn.starts,0);
  s.overlay.backingWindowVisible=true; s.startEntryFrame();
  assert.equal(s.slideIn.starts,1);
  assert.equal(s.entryPhase,'sliding');
  assert.equal(s.writes.filter(x=>x.action==='opened').length,1);
  s.startEntryFrame();
  assert.equal(s.slideIn.starts,1,'subsequent frames must not restart animation');
  s.receive(JSON.stringify({type:'input-reset'}));
  assert.equal(s.entryPhase,'sliding','controller recovery must not restart presentation');
  s.closeMenu(); s.startEntryFrame();
  assert.equal(s.entryPhase,'hidden');
  assert.equal(s.panel.entryProgress,0);
  assert.equal(s.slideIn.starts,1,'late frames must not reopen a closed menu');
  checks += 14;
}
const cancelled = fixture();
cancelled.entryPhase='await-frame'; cancelled.closeMenu(); cancelled.startEntryFrame();
assert.equal(cancelled.slideIn.starts,0);
assert.equal(cancelled.writes.filter(x=>x.action==='opened').length,0);
assert.match(source,/target: panel\.Window\.window[\s\S]*?onFrameSwapped\(\)/);
console.log(`PASS: ${checks + 3} entry geometry/lifecycle assertions using actual QML source.`);

// A visible menu retains its lease through the reverse animation.
for (const progress of [.15,.5,1]) {
  const s=fixture('audio');s.panel.entryProgress=progress;s.entryPhase='settled';s.activeTab=2;
  s.closeMenu();
  assert.equal(s.menuOpen,true);assert.equal(s.closing,true);assert.equal(s.entryPhase,'closing');
  assert.equal(s.slideOut.starts,1);assert.equal(s.slideOut.from,progress);
  assert.equal(s.slideOut.duration,Math.max(1,Math.round(180*progress)));
  assert.equal(s.writes.filter(x=>x.action==='close').length,0,'input stays captured while the panel is visible');
  const originalPage=s.page,originalSelected=s.selected;
  s.input('ui_accept',1);s.move(1);s.activate();s.back();s.switchTab(1);s.notificationShortcut(true);s.closeMenu();
  assert.equal(s.page,originalPage);assert.equal(s.selected,originalSelected);assert.equal(s.activeTab,2);
  assert.equal(s.slideOut.starts,1,'duplicate requests cannot restart the close');
  s.closeMenu(true);
  assert.equal(s.menuOpen,false);assert.equal(s.panel.entryProgress,0);
  assert.equal(s.writes.filter(x=>x.action==='close').length,1);
  assert.equal(s.writes.at(-1).page,'audio');assert.equal(s.writes.at(-1).tab,2);
  s.closeMenu(true);assert.equal(s.writes.filter(x=>x.action==='close').length,1,'late completion is ignored');
  s.receive(JSON.stringify({type:'helper-idle',protocol:1}));
  assert.equal(s.helperIdle,true);assert.equal(s.closing,false);
  s.openMenu();s.closeMenu(true);assert.equal(s.opening,true,'old animation cannot close a new generation');
}
const interrupted=fixture();interrupted.panel.entryProgress=.4;interrupted.entryPhase='sliding';interrupted.closeMenu();
assert.equal(interrupted.slideOut.from,.4);interrupted.receive(JSON.stringify({type:'ready',foreground:'library'}));
assert.equal(interrupted.entryPhase,'closing','late readiness cannot reopen the closing panel');
interrupted.closeMenu(true);assert.equal(interrupted.writes.filter(x=>x.action==='close').length,1);
const handoff=fixture();handoff.panel.entryProgress=1;handoff.entryPhase='settled';
handoff.queueStoreAction('launch','dukkan','0');assert.equal(handoff.storeAction.running,false);
handoff.closeMenu(true);assert.equal(handoff.storeAction.running,false,'hide completion is not a restoration acknowledgement');
handoff.receive(JSON.stringify({type:'helper-idle',protocol:1}));assert.equal(handoff.storeAction.running,true);
const exit=fixture('exit');exit.panel.entryProgress=1;exit.requestExitGame();
exit.receiveExitResult(JSON.stringify({type:'exit-result',ok:true}));assert.equal(exit.menuOpen,true);assert.equal(exit.closing,true);
exit.finishExitAction(0);exit.closeMenu(true);assert.equal(exit.menuOpen,false);
const preempted=fixture();preempted.panel.entryProgress=1;preempted.closeMenu();
preempted.receive(JSON.stringify({type:'closing'}));preempted.closeMenu(true);
assert.equal(preempted.menuOpen,false);assert.equal(preempted.writes.length,0,'preemption is restored by the helper without a duplicate close');
const deadline=source.match(/id: closeDeadline[\s\S]*?onTriggered: ([^\n]+)/)[1];
const stalled=fixture();stalled.panel.entryProgress=1;stalled.closeMenu();vm.runInContext(deadline,stalled);
assert.equal(stalled.menuOpen,false);assert.equal(stalled.writes.filter(x=>x.action==='close').length,1,'bounded fallback releases a non-rendering close');
assert.match(source,/id: slideOut[\s\S]*?to: 0[\s\S]*?easing.type: Easing.InCubic/);
console.log('PASS: reverse close, input ownership, interrupted opening, duplicate events, late readiness, store/exit handoff and bounded fallback.');

const unmapped=fixture();unmapped.panel.entryProgress=1;unmapped.closeMenu();unmapped.backingWindowVisible=false;
const backingChange=source.match(/onBackingWindowVisibleChanged: \{([\s\S]*?)^        \}/m)[1];
vm.runInContext('(function(){'+backingChange+'})()',unmapped);
assert.equal(unmapped.menuOpen,false);assert.equal(unmapped.writes.filter(x=>x.action==='close').length,1,'unmapping during close cannot strand input ownership');
