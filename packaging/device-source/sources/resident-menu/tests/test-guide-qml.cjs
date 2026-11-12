const assert=require('node:assert/strict');
const vm=require('node:vm');
const fs=require('node:fs');
const {fixture,source}=require('./menu-controller-regression.cjs');
let checks=0;
const toggleOffset=source.indexOf('        function toggle(): string {');
const toggleStart=source.indexOf('{',toggleOffset)+1;
const toggle=source.slice(toggleStart,source.indexOf('\n        }',toggleStart));
function state(){const value=fixture();value.root=value;value.menuGeneration=0;value.resetMenuSession();value.helperIdle=true;return value;}
const value=state();
vm.runInContext('(function(){'+toggle+'\n})()',value);
assert.equal(value.opening,true);assert.equal(value.menuGeneration,1);checks++;
value.receive(JSON.stringify({type:'ready',foreground:'game',display:true,controllers:1,devices:[]}));
const first=value.menuGeneration;
vm.runInContext('(function(){'+toggle+'\n})()',value);
assert.equal(value.closing,true);
value.receive(JSON.stringify({type:'helper-idle',protocol:1}));
vm.runInContext('(function(){'+toggle+'\n})()',value);
assert.equal(value.menuGeneration,first+1,'new lease receives a distinct generation');checks++;
for(const transition of ['opening','closing']){
  const s=state();s[transition]=true;vm.runInContext('(function(){'+toggle+'\n})()',s);
  assert.equal(s.writes.length,0,'toggle during a transition cannot create an overlapping lease');checks++;
}
const busy=state();busy.exitBusy=true;vm.runInContext('(function(){'+toggle+'\n})()',busy);
assert.equal(busy.menuStatus(),'busy');assert.equal(busy.writes.length,0);checks++;
const closeOffset=source.indexOf('        function closeSession(key: string): string {');
const closeStart=source.indexOf('{',closeOffset)+1;
const closeBody=source.slice(closeStart,source.indexOf('\n        }',closeStart));
function closeSession(s,key){s.key=key;vm.runInContext('(function(){'+closeBody+'\n})()',s);}
for(const key of ['previous-instance:1','test:0','test:2']){
  const s=fixture();s.root=s;s.menuGeneration=1;closeSession(s,key);
  assert.equal(s.menuOpen,true,'stale Guide release cannot close a different lease');checks++;
}
const matched=fixture();matched.root=matched;matched.menuGeneration=1;
closeSession(matched,'test:1');assert.equal(matched.closing,true);checks++;
for(const transition of ['exitBusy','opening','closing']){
  const s=fixture();s.root=s;s.menuGeneration=1;s[transition]=true;
  closeSession(s,'test:1');assert.equal(s.writes.length,0);checks++;
}
for(const page of ['reboot','shutdown']){
  const s=fixture(page);s.requestExitGame();
  assert.equal(s.sessionAction,page);
  assert.equal(s.exitBusy,true);assert.equal(s.exitAction.running,true);
  s.receiveExitResult(JSON.stringify({type:'power-progress',message:'Power pending'}));
  assert.equal(s.menuOpen,true,'power ack must not be treated as completion');
  s.receiveExitResult(JSON.stringify({type:'exit-result',ok:false,message:'Permission denied'}));
  s.finishExitAction(1);
  assert.equal(s.exitBusy,false);assert.equal(s.exitNotice,'Permission denied');checks++;
}
const pageSource=fs.readFileSync(require('node:path').join(__dirname,'../qml/ExitGamePage.qml'),'utf8');
assert.match(pageSource,/model: \[exitPage.cancelLabel, exitPage.confirmLabel\]/);
assert.match(source,/cancelLabel: root.page === "exit" \? "Keep playing" : "Cancel"/);
console.log(`PASS: ${checks} Guide IPC, lease generation and power QML scenarios.`);

// Every section uses the same close/lease-restoration handshake, with origin
// recorded before page shutdown can mutate page state.
for (const page of ['', 'settings', 'audio', 'network', 'bluetooth', 'controllers',
                    'display', 'appearance', 'notifications', 'notification-settings',
                    'storefronts', 'store-close', 'exit', 'reboot', 'shutdown']) {
    const s = fixture(page); s.activeTab = page === 'notifications' ? 1 : 2;
    if (page === 'bluetooth') s.bluetoothPageLoader.item = {shutdown:()=>{s.page='settings';}};
    s.closeMenu();
    const command = s.writes.filter(x=>x.action === 'close');
    assert.equal(command.length,1,'one restoration request per close');
    assert.equal(command[0].page,page,'origin survives section shutdown');
    assert.equal(command[0].tab,s.activeTab);
    assert.equal(s.menuOpen,false); assert.equal(s.closing,true);
    assert.equal(s.helperIdle,false,'restoration must be acknowledged before idle');
    s.receive(JSON.stringify({type:'helper-idle',protocol:1}));
    assert.equal(s.helperIdle,true); assert.equal(s.closing,false);
}
console.log('PASS: close origin and restoration handshake across 15 menu sections.');
