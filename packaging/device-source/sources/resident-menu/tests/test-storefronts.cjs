const assert=require('node:assert/strict');
const {fixture,source}=require('./menu-controller-regression.cjs');
const vm=require('node:vm');
let checks=0;
for(const action of ['launch','home','close']) {
  const s=fixture();
  s.queueStoreAction(action,'dukkan',action==='launch'?'0':'9');
  assert.equal(s.menuOpen,false); assert.equal(s.closing,true);
  assert.equal(s.storeAction.running,false,'launch must await input/focus restoration');
  s.queueStoreAction('launch','steam','0');
  assert.equal(s.pendingStoreAction.id,'dukkan','duplicate click cannot replace the queued request');
  s.receive(JSON.stringify({type:'closed'}));
  assert.equal(s.storeAction.running,false,'closed alone is not the idle acknowledgement');
  s.receive(JSON.stringify({type:'helper-idle',protocol:1}));
  assert.equal(s.storeAction.running,true);
  assert.equal(s.storeRequest.id,'dukkan'); assert.equal(s.storeRequest.action,action);
  assert.equal(s.exitBusy,true); assert.equal(s.menuOpen,false);
  assert.equal(s.pendingStoreAction,null);
  s.receive(JSON.stringify({type:'ready',foreground:'storefront',storefront:'dukkan'}));
  assert.equal(s.menuOpen,false,'late readiness cannot reopen the layer during launch'); checks++;
}
const s=fixture(); s.resetMenuSession(); s.openMenu();
s.receive(JSON.stringify({type:'ready',foreground:'storefront',storefront:'steam',storeGeneration:'18446744073709551615',storeGame:true,controllers:1}));
assert.equal(s.storefrontId,'steam'); assert.equal(s.storeGame,true);
assert.equal(s.storeGeneration,'18446744073709551615','generation remains exact outside JS safe-integer range'); checks++;
const expression=source.match(/readonly property var mainEntries: ([\s\S]*?)\n    readonly property var storefrontEntries/)[1];
for(const [foreground,id,game] of [['library','',false],['storefront','dukkan',false],['storefront','dukkan',true],['storefront','steam',false],['storefront','steam',true],['game','steam',true],['game','',false]]) {
 const state=fixture();state.foreground=foreground;state.storefrontId=id;state.storeGame=game;
 const entries=vm.runInContext(expression,state);const keys=entries.map(e=>e.key);
 assert.equal(keys.includes('storefronts'),foreground==='library');
 assert.equal(keys.includes('exit'),foreground==='game' && !id);
 assert.equal(keys.includes('store-close'),!!id && (!game || id==='steam'));
 assert.equal(keys.includes('store-home'),!!id && !game);checks++;
}
// Selecting recovery only opens confirmation. Cancel performs no shutdown.
const recovery=fixture();recovery.foreground='storefront';recovery.storefrontId='steam';
recovery.storeGame=true;recovery.storeGeneration='18446744073709551615';
recovery.mainEntries=vm.runInContext(expression,recovery);
recovery.selected=recovery.mainEntries.findIndex(e=>e.key==='store-close');
assert.equal(recovery.mainEntries[recovery.selected].label,'Close Steam');
recovery.activate();assert.equal(recovery.page,'store-close');
assert.equal(recovery.pendingStoreAction,null);assert.equal(recovery.storeAction.running,false);
const detail=source.match(/^                detail: (.*)$/m)[1];
assert.match(vm.runInContext(detail,recovery),/Steam and its running game/);
assert.match(vm.runInContext(detail,recovery),/Unsaved progress/);
recovery.back();assert.equal(recovery.page,'');assert.equal(recovery.pendingStoreAction,null);checks++;
recovery.selected=recovery.mainEntries.findIndex(e=>e.key==='store-close');recovery.activate();
recovery.requestExitGame();assert.equal(recovery.pendingStoreAction.action,'close');
assert.equal(recovery.pendingStoreAction.id,'steam');assert.equal(recovery.storeAction.running,false);
recovery.receive(JSON.stringify({type:'closed'}));assert.equal(recovery.storeAction.running,false);
recovery.receive(JSON.stringify({type:'helper-idle',protocol:1}));
assert.equal(recovery.storeAction.running,true);assert.equal(recovery.storeRequest.generation,'18446744073709551615');
assert.equal(recovery.storeRequest.id,'steam');assert.equal(recovery.storeRequest.action,'close');checks++;
console.log(`PASS: ${checks} storefront handoff, identity and contextual action scenarios.`);
