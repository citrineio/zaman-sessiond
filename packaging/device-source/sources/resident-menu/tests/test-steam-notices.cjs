const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
const source = fs.readFileSync(path.join(__dirname, '../qml/shell.qml'), 'utf8');
const array = source.match(/readonly property var notificationSettings: (\[[\s\S]*?\n    \])/)[1];
const rows = vm.runInNewContext(array, {notifications: {preferences: {dnd: false, steamStartup: true, steamReminder: false}}});
assert.equal(rows.length, 3);
assert.equal(new Set(rows.map(row => row.preference)).size, 3);
assert.equal(rows.find(row => row.preference === 'steamStartup').label, 'Steam startup notice');
const branch = source.match(/if \(page === "notification-settings"\) \{([\s\S]*?)\n        \}/)[1];
for (let i = 0; i < rows.length; i++) {
  const sent = [];
  const preferences = {dnd: false, steamStartup: true, steamReminder: false};
  const context = {entries: rows, selected: i, notifications: {preferences, send: value => sent.push(value)}};
  vm.runInNewContext('(function(){' + branch + '})()', context);
  assert.equal(sent.length, 1);
  assert.equal(sent[0][rows[i].preference], !preferences[rows[i].preference]);
  assert.equal(Object.keys(sent[0]).length, 2);
}
console.log('PASS: three visible notification controls send independent preference updates');
