const assert=require('node:assert/strict');
const vm=require('node:vm');
const fs=require('node:fs');
const path=require('node:path');
const {fixture,source}=require('./menu-controller-regression.cjs');
let checks=0;
const s=fixture();
s.input('ui_r1',1);assert.equal(s.activeTab,1);assert.equal(s.page,'notifications');checks++;
for(let i=0;i<50;i++)s.input('ui_r1',1);
assert.equal(s.activeTab,1,'holding R1 switches once');checks++;
s.input('ui_r1',0);s.input('ui_r1',1);assert.equal(s.activeTab,2);checks++;
s.input('ui_r1',0);s.input('ui_r1',1);assert.equal(s.activeTab,0);checks++;
s.input('ui_l1',1);assert.equal(s.activeTab,2,'left wraps');checks++;
for(const page of ['network','bluetooth','audio','display','reboot','shutdown','exit','notification-settings','store-close']){
 const t=fixture(page);t.input('ui_r1',1);assert.equal(t.page,'notifications');
 t.input('ui_l1',1);assert.equal(t.page,page,'tab returns to its previous section');checks++;
}
const blocked=fixture('notifications');blocked.activeTab=1;blocked.notificationsPage.blocked=true;
blocked.switchTab(1);assert.equal(blocked.activeTab,2,'a history dialog does not trap shoulder navigation');checks++;
const reconnect=fixture('notifications');reconnect.activeTab=1;
reconnect.receive(JSON.stringify({type:'input-reset'}));assert.equal(reconnect.activeTab,1);assert.equal(reconnect.page,'notifications');checks++;
const home=fixture();home.notifications.popups=[{}];home.input('ui_context',1);
assert.equal(home.actions[0].action,'dismiss-visible','Observed device X event dismisses popups');checks++;
const history=fixture('notifications');history.activeTab=1;history.input('ui_context',1);assert.equal(history.actions[0],'dismiss');checks++;
history.input('ui_action',1);assert.equal(history.actions[1],'clear','Observed device Y event clears history');checks++;
for(const page of ['network','bluetooth','display','reboot','shutdown','audio']){
 const t=fixture(page);t.input('ui_action',1);t.input('ui_context',1);assert.equal(t.actions.length,0);checks++;
}
const restore=fixture();restore.selected=1;restore.menuList.contentY=80;restore.switchTab(1);restore.switchTab(1);restore.switchTab(1);
assert.equal(restore.selected,1);assert.equal(restore.menuList.contentY,80);checks++;
const close=fixture('settings');close.activeTab=2;close.back();assert.equal(close.closing,true);checks++;
const stale=fixture();let callbacks=[];stale.Qt.callLater=fn=>callbacks.push(fn);stale.switchTab(1);stale.switchTab(1);stale.menuGeneration++;
stale.menuList.contentY=99;callbacks.forEach(fn=>fn());assert.equal(stale.menuList.contentY,99);checks++;
// Exercise the actual notification page JavaScript for concurrent arrivals/confirmation.
const pageSource=fs.readFileSync(path.join(__dirname,'../qml/NotificationsPage.qml'),'utf8');
const fns=pageSource.match(/^    function \w+\([^\n]*\) \{[^\n]*\}$|^    function \w+\([^\n]*\) \{[\s\S]*?^    \}/gm);
const commands=[];
const p={records:[{id:'old',title:'First'}],selectedId:'old',detail:null,clearIds:[],confirming:false,confirmSelected:0,visible:true,
 client:{available:true,send:m=>commands.push(m)},Qt:{callLater:fn=>fn()},ListView:{Contain:0},history:{positionViewAtIndex:()=>{}},
 details:{contentY:0,contentHeight:100,height:100},uiScale:1};
Object.defineProperties(p,{selected:{get:()=>p.records.find(r=>r.id===p.selectedId)},blocked:{get:()=>p.confirming||p.detail!==null}});
const visibleItems=[];
p.visibleRecords={get count(){return visibleItems.length},get:i=>visibleItems[i],
 remove:i=>visibleItems.splice(i,1),insert:(i,r)=>visibleItems.splice(i,0,r),
 move:(from,to)=>visibleItems.splice(to,0,visibleItems.splice(from,1)[0]),setProperty:(i,k,v)=>visibleItems[i][k]=v};
Object.defineProperty(p.client,'records',{get:()=>p.records});
vm.createContext(p);vm.runInContext(fns.join('\n'),p);p.sync();
p.records.unshift({id:'new',title:'New'});p.sync();assert.equal(p.selectedId,'old');checks++;
p.clear();p.records.unshift({id:'newer'});p.confirmSelected=1;p.activate();
assert.deepEqual(Array.from(commands[0].ids),['new','old']);checks++;
p.activate();assert.equal(p.detail.id,'old');p.records.unshift({id:'latest'});p.sync();assert.equal(p.detail.id,'old');checks++;
assert.equal(p.back(),true);assert.equal(p.detail,null);checks++;
p.records=p.records.filter(record=>record.id!=='old');p.sync();assert.equal(p.selectedId,'new','dismissed last item selects its nearest neighbor');checks++;
p.selectedId='newer';p.records=p.records.filter(record=>record.id!=='newer');p.sync();assert.equal(p.selectedId,'new','dismissing a middle item preserves list position');checks++;
p.records=[];p.sync();assert.equal(p.selectedId,'');assert.equal(visibleItems.length,0);checks++;
p.records=[{id:'arrived-during-exit',title:'New arrival'}];p.sync();
assert.equal(p.selectedId,'arrived-during-exit');assert.equal(visibleItems[0].recordId,'arrived-during-exit');checks++;
assert.match(source,/Qt\.ControlModifier/);assert.match(source,/Qt\.Key_PageUp/);checks++;
const toastSource=fs.readFileSync(path.join(__dirname,'../qml/NotificationToastStack.qml'),'utf8');
const sync=toastSource.match(/^    function syncCards\(\) \{[\s\S]*?^    \}/m)[0];
let now=0;const items=[];
const renderer={popups:[],Date:{now:()=>now},hasClosingCards:false,
 cards:{get count(){return items.length},get:i=>items[i],setProperty:(i,k,v)=>items[i][k]=v,append:r=>items.push(r),remove:i=>items.splice(i,1)}};
vm.createContext(renderer);vm.runInContext(sync,renderer);
renderer.popups=[{id:'a',title:'First'},{id:'b',title:'Second'}];renderer.syncCards();
assert.equal(items.length,2);checks++;
const original=items[0];renderer.popups=[{id:'a',title:'Replacement'},{id:'b',title:'Second'}];renderer.syncCards();
assert.equal(items[0],original);assert.equal(items[0].payload.title,'Replacement');checks++;
renderer.popups=[{id:'c',title:'Critical'},{id:'b',title:'Second'}];renderer.syncCards();
assert.equal(items.length,2);assert.equal(items[0].isClosing,true);assert.equal(renderer.hasClosingCards,true);checks++;
now=150;renderer.syncCards();assert.equal(items.length,2);assert.ok(items.some(r=>r.rid==='c'));assert.equal(renderer.hasClosingCards,false);checks++;
renderer.popups=[];renderer.syncCards();now=300;renderer.syncCards();assert.equal(items.length,0);checks++;
console.log(`PASS: ${checks} Sprint 4 tabs, face mapping, guards, stable selection and captured-clear scenarios.`);
