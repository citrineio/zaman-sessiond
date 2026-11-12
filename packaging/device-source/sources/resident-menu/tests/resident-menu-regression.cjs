// Exercise the actual QML lifecycle and handlers, including acquisition races.
const assert = require('node:assert/strict');
const vm = require('node:vm');
const {fixture, source} = require('./menu-controller-regression.cjs');
let checks = 0;
function handler(marker) {
  const offset = source.indexOf(marker);
  assert.notEqual(offset, -1);
  const start = source.indexOf('{', offset) + 1;
  return source.slice(start, source.indexOf('\n    }', start));
}
const startup = handler('    Component.onCompleted:');
// Process callback closes at eight-space indentation.
const exitOffset = source.indexOf('        onExited: (exitCode, exitStatus) => {', source.indexOf('        id: bridge'));
const exitStart = source.indexOf('{', exitOffset) + 1;
const exited = source.slice(exitStart, source.indexOf('\n        }', exitStart));
function idle() {
  const state = fixture();
  state.root = state;
  state.helperPath = '/test/helper';
  state.Quickshell = {env:()=> '0', watchFiles:true};
  state.resetMenuSession();
  state.bridge.running = false;
  state.starts = 0;
  let running = false;
  Object.defineProperty(state.bridge, 'running', {
    get:()=>running,
    set:value=>{ if (value && !running) state.starts++; running = value; }
  });
  vm.runInContext(startup, state);
  assert.equal(state.menuStatus(), 'hidden');
  assert.equal(state.starts, 1, 'resident boot starts one helper without opening the menu');
  state.receive(JSON.stringify({type:'helper-idle', protocol:1}));
  assert.equal(state.helperIdle, true);
  assert.equal(state.quit, 0);
  return state;
}
function exitHelper(state, code = 0) {
  state.bridge.running = false;
  state.exitCode = code;
  vm.runInContext(exited, state);
  assert.equal(state.menuStatus(), 'hidden');
  assert.equal(state.quit, 0, 'closing a lease must not exit the renderer');
  checks++;
}
function helperIdle(state) {
  state.receive(JSON.stringify({type:'helper-idle', protocol:1}));
  assert.equal(state.menuStatus(), 'hidden');
  assert.equal(state.helperIdle, true);
  assert.equal(state.bridge.running, true);
  assert.equal(state.quit, 0);
  checks++;
}
function ready(state) {
  state.receive(JSON.stringify({type:'ready', foreground:'game', display:true,
    controllers:1, devices:[{id:'controller'}]}));
}
const boot = idle();
boot.closeMenu();
assert.equal(boot.writes.length, 0, 'closing a hidden renderer is a no-op');
ready(boot);
assert.equal(boot.menuOpen, false, 'unsolicited readiness cannot reveal the overlay');
checks++;
// Repeat full sessions without restarting either process or overlapping leases.
for (let cycle=0; cycle<50; cycle++) {
  assert.equal(boot.openMenu(), 'opening');
  assert.equal(boot.starts, 1, 'all sessions reuse the same helper');
  assert.equal(boot.writes.at(-1).action, 'open');
  for (let duplicate=0; duplicate<5; duplicate++) assert.equal(boot.openMenu(), 'opening');
  assert.equal(boot.menuOpen, false, 'input lease must precede presentation');
  ready(boot);
  assert.equal(boot.menuStatus(), 'open');
  assert.equal(boot.openMenu(), 'open', 'repeated shortcut must not toggle close');
  boot.input('ui_back', 1);
  assert.equal(boot.menuStatus(), 'closing');
  const writes = boot.writes.length;
  boot.closeMenu();
  assert.equal(boot.writes.length, writes, 'duplicate close must not repeat restoration');
  assert.equal(boot.openMenu(), 'closing', 'new input lease must wait for restoration');
  boot.receive(JSON.stringify({type:'closed'}));
  assert.equal(boot.menuStatus(), 'closing', 'closed JSON alone does not acknowledge the idle protocol');
  helperIdle(boot);
  assert.equal(boot.ready, false);
  assert.equal(boot.controllerDevices.length, 0);
  assert.equal(Object.keys(boot.pressed).length, 0);
  checks++;
}
// Close before acquisition completes; late ready must never reopen the layer.
const cancelled = idle();
cancelled.openMenu();
cancelled.closeMenu();
ready(cancelled);
assert.equal(cancelled.menuOpen, false);
assert.equal(cancelled.menuStatus(), 'closing');
helperIdle(cancelled);
cancelled.openMenu(); ready(cancelled);
assert.equal(cancelled.menuOpen, true, 'a new opening can recover after cancellation');
checks++;
// Production Guide/Transfer preemption ends this lease, keeping the renderer.
const preempted = idle();
preempted.openMenu(); ready(preempted);
preempted.receive(JSON.stringify({type:'closing'}));
assert.equal(preempted.menuOpen, false);
assert.equal(preempted.writes.filter(x=>x.action==='close').length, 0, 'helper preemption is already performing restoration');
helperIdle(preempted);
checks++;
// A failed helper hides the overlay and leaves future openings available.
const failed = idle();
failed.openMenu(); ready(failed);
failed.receive(JSON.stringify({type:'error', message:'helper error'}));
assert.equal(failed.menuOpen, false);
exitHelper(failed, 1);
failed.openMenu();
failed.receive(JSON.stringify({type:'helper-idle', protocol:1}));
ready(failed);
assert.equal(failed.menuOpen, true);
checks++;
// Opening while the worker is still booting waits for its idle handshake.
const early = fixture(); early.root=early;
early.resetMenuSession(); early.helperIdle=false; early.bridge.running=true;
early.openMenu();
assert.equal(early.writes.length, 0);
early.receive(JSON.stringify({type:'helper-idle', protocol:1}));
assert.equal(early.writes.filter(x=>x.action==='open').length, 1);
assert.equal(early.opening, true);
checks++;
const earlyCancel = fixture(); earlyCancel.root=early;
earlyCancel.resetMenuSession(); earlyCancel.helperIdle=false; earlyCancel.bridge.running=true;
earlyCancel.openMenu(); earlyCancel.closeMenu();
earlyCancel.receive(JSON.stringify({type:'helper-idle', protocol:1}));
assert.equal(earlyCancel.writes.filter(x=>x.action==='open').length, 0);
assert.equal(earlyCancel.menuStatus(), 'hidden');
checks++;
// A session error returns to idle without killing either process.
const sessionError=idle(); sessionError.openMenu(); ready(sessionError);
sessionError.receive(JSON.stringify({type:'error',message:'foreground rejected'}));
helperIdle(sessionError);
sessionError.openMenu(); ready(sessionError);
assert.equal(sessionError.starts, 1);
checks++;
console.log(`PASS: ${checks} resident lifecycle cases, including 50 open/close cycles.`);
