const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');
const path = require('node:path');
const {fixture, source} = require('./menu-controller-regression.cjs');

const keyboard = fs.readFileSync(path.join(__dirname, '../qml/PasswordKeyboard.qml'), 'utf8');
assert.match(keyboard, /QWERTYUIOPASDFGHJKL-ZXCVBNM,\.\/1234567890/);
assert.match(keyboard, /qwertyuiopasdfghjkl-zxcvbnm,\.\/1234567890/);
assert.match(keyboard, /view === 2 \? symbols : letters/);
assert.match(keyboard, /Y: change keyboard/);
const cycleSource = keyboard.match(/function cycleView\(\) \{[^}]+\}/)?.[0];
assert.ok(cycleSource);
const keyboardState = vm.createContext({view:0, digitsOnly:false});
vm.runInContext(cycleSource, keyboardState);
for (const expected of [1, 2, 0]) {
  vm.runInContext('cycleView()', keyboardState);
  assert.equal(keyboardState.view, expected);
}
keyboardState.digitsOnly = true;
vm.runInContext('cycleView()', keyboardState);
assert.equal(keyboardState.view, 0, 'numeric passkeys stay numeric');

function timerBody(id) {
  const start = source.indexOf(`        id: ${id}`);
  assert.notEqual(start, -1);
  const trigger = source.indexOf('        onTriggered: {', start);
  assert.notEqual(trigger, -1);
  return source.slice(trigger + '        onTriggered: {'.length,
                      source.indexOf('\n        }', trigger));
}

for (const [page, flag] of [['network', 'editingPassword'], ['bluetooth', 'editingCode']]) {
  const state = fixture(page);
  const steps = [];
  state[page === 'network' ? 'networkPageLoader' : 'bluetoothPageLoader'].item = {
    [flag]: true, horizontal: direction => steps.push(direction), move: () => {}
  };
  state.input('ui_right', 1);
  assert.deepEqual(steps, [1]);
  assert.equal(state.heldHorizontal, 1);
  state.input('ui_right', 1);
  assert.deepEqual(steps, [1], 'duplicate press does not create another movement');
  vm.runInContext(timerBody('horizontalDelay'), state);
  vm.runInContext(timerBody('horizontalStep'), state);
  assert.deepEqual(steps, [1, 1, 1], 'held key repeats in both timer phases');
  state.input('ui_right', 0);
  assert.equal(state.heldHorizontal, 0);
  vm.runInContext(timerBody('horizontalStep'), state);
  assert.deepEqual(steps, [1, 1, 1], 'release stops repeated movement');
  let cycles = 0;
  state[page === 'network' ? 'networkPageLoader' : 'bluetoothPageLoader'].item.cycleKeyboard = () => cycles++;
  state.input('ui_action', 1);
  state.input('ui_context', 1);
  assert.equal(cycles, 1, 'two mapped face events during one press cycle once');
  state.input('ui_action', 0);
  state.input('ui_context', 0);
  state.input('ui_context', 1);
  assert.equal(cycles, 2, 'next press cycles again');
}
console.log('PASS: QWERTY, held movement, and keyboard view cycling for Wi-Fi and Bluetooth.');
