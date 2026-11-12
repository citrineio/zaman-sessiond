const fs=require('node:fs');
const path=require('node:path');
const vm=require('node:vm');
const assert=require('node:assert/strict');
const qml=fs.readFileSync(path.join(__dirname,'../qml/MenuTabBar.qml'),'utf8');
const fn=qml.match(/^    function tabWidth\(index\) \{[\s\S]*?^    \}/m)[0];
let checks=0;
for(const scale of [0.65,1,1.333,1.4])for(const width of [220,480,636,900])for(const controller of [true,false]){
 const controlWidth=Math.round((controller?40:68)*scale);
 const edgeGap=Math.round(8*scale),tabGap=Math.round(7*scale);
 const availableWidth=Math.max(0,width*scale-2*controlWidth-2*edgeGap-2*tabGap);
 const s={uiScale:scale,availableWidth,currentIndex:0};vm.createContext(s);vm.runInContext(fn,s);
 const results=[];
 for(let active=0;active<3;active++){
  s.currentIndex=active;const widths=[0,1,2].map(i=>s.tabWidth(i));results.push(widths);
  assert.ok(widths.every(w=>Number.isFinite(w)&&w>=0));
  assert.ok(Math.abs(widths.reduce((a,b)=>a+b,0)-availableWidth)<1e-6,'tabs fill available width exactly');
  if(availableWidth>379*scale)assert.ok(widths[active]>[104,171,104][active]*scale);
  checks++;
 }
 assert.equal(results[0][1],results[2][1],'inactive Notifications keeps its default width');
 assert.equal(results[1][0],results[2][0],'inactive Home keeps its default width');
 checks+=2;
}
assert.match(qml,/Behavior on width[\s\S]*duration: 180/);
assert.match(qml,/MenuShoulderGlyph/);
assert.doesNotMatch(qml,/text: "[LR]1"/);
const page=fs.readFileSync(path.join(__dirname,'../qml/NotificationsPage.qml'),'utf8');
const binding=page.match(/readonly property var records: ([^\n]+)/)[1];
const records=vm.runInNewContext(binding,{liveImports:[],liveScrapes:[],liveTransactions:[],client:{records:[{id:'kept',dismissed:false},{id:'hidden',dismissed:true}]}});
assert.deepEqual(Array.from(records,r=>r.id),['kept']);checks++;
const shell=fs.readFileSync(path.join(__dirname,'../qml/shell.qml'),'utf8');
assert.match(shell,/notificationShortcut\(event === "ui_action"\)/,'matches the reported effective controller mapping');
assert.match(shell,/notificationShortcut\(event.key === Qt.Key_Y\)/,'keyboard X and Y retain their explicit meanings');
// Evaluate the real contextual legend for controller and keyboard, including OSK.
const guide=shell.slice(shell.indexOf('id: navigationGuide'),shell.indexOf('id: navigationGuide')+6000);
const legend=guide.match(/model: (\[[\s\S]*?\n\s*\])/)[1];
const diamondVisible=guide.match(/MenuButtonDiamond \{\s*visible: ([^\n]+)/)[1];
const keyVisible=guide.match(/MenuText \{\s*visible: ([^\n]+)/)[1];
for(const mode of ['navigate','adjust','text'])for(const controllerCount of [0,1]){
 const context={navigationGuide:{adjusting:mode==='adjust',textEntry:mode==='text'},
  root:{page:mode==='adjust'?'audio':'notifications',controllerCount},
  audioPageLoader:{item:{rows:[{key:'output'}],selected:0}},
  notifications:{available:true,popups:[],records:[{}]},notificationsPage:{hasItems:true,blocked:false,selected:{}}};
 const entries=vm.runInNewContext('('+legend+')',context);
 assert.equal(entries[0].key,mode==='adjust'?'←→':mode==='text'?'↑↓←→':'↑↓');
 for(const modelData of entries){
  const diamond=vm.runInNewContext(diamondVisible,{...context,modelData});
  const key=vm.runInNewContext(keyVisible,{...context,modelData});
  assert.equal(diamond,modelData.directional!==true && controllerCount>0);
  assert.equal(key,modelData.directional===true || controllerCount===0);
  assert.doesNotMatch(modelData.label,/[↑↓←→]/,'arrows belong only to the directional key');
  if(!modelData.directional)assert.ok(modelData.faces.length===1,'action glyph has one face');
 }
 checks++;
}
assert.match(guide,/activeFaces: modelData.faces \|\| \[\]/,'hidden directional glyph has safe empty faces');
const remove=page.slice(page.indexOf('remove: Transition'),page.indexOf('spacing: page.px(7)'));
assert.match(remove,/property:"dismissOffset"; to:page.px\(96\); duration:180; easing.type:Easing.OutCubic/);
assert.match(remove,/property:"opacity"; to:0; duration:180/);
assert.doesNotMatch(remove,/property:"height"/,'exit keeps card shape while sliding');
assert.match(page,/transform: Translate \{ x: notificationRow.dismissOffset \}/);
assert.match(page,/enabled: !notificationRow.retiring/,'exiting rows cannot receive a stale click');
console.log(`PASS: ${checks} adaptive tab width, dismissed-list and contextual legend cases, plus glyph/transition/controller routing checks.`);
