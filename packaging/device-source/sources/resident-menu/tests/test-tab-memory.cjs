const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
const vm=require('node:vm');
const {fixture,source}=require('./menu-controller-regression.cjs');
let checks=0;
// The user's exact example: Audio -> Notifications -> Home restores Audio.
const s=fixture('audio');s.returnPage='';s.mainSelected=1;s.pageEntry={key:'audio',label:'Audio'};
let savedAudio={selected:2,scroll:72,candidateName:'HDMI'};
s.audioPageLoader.item={stopHorizontal:()=>{},location:()=>({...savedAudio}),restoreLocation:state=>savedAudio={...state}};
s.switchTab(1);assert.equal(s.page,'notifications');checks++;
s.switchTab(-1);assert.equal(s.page,'audio');assert.equal(s.returnPage,'');
assert.equal(savedAudio.selected,2);assert.equal(s.pageEntry.label,'Audio');checks++;
// Home and Settings can both contain Audio without sharing the selected control.
s.switchTab(-1);s.page='audio';s.returnPage='settings';savedAudio.selected=1;
s.switchTab(1);assert.equal(s.page,'audio');assert.equal(s.returnPage,'');assert.equal(savedAudio.selected,2);checks++;
s.switchTab(-1);assert.equal(s.returnPage,'settings');assert.equal(savedAudio.selected,1);checks++;
// Section and root-row memory survive closing and a new input lease.
s.hideMenu();s.resetMenuSession();s.openMenu();
s.receive(JSON.stringify({type:'ready',foreground:'game',display:true,controllers:1}));
assert.equal(s.activeTab,2);assert.equal(s.page,'audio');assert.equal(savedAudio.selected,1);checks++;
const root=fixture();root.selected=1;root.menuList.contentY=64;
root.closeMenu();root.receive(JSON.stringify({type:'helper-idle',protocol:1}));root.openMenu();
root.receive(JSON.stringify({type:'ready',foreground:'game',display:true,controllers:1}));
assert.equal(root.selected,1);assert.equal(root.menuList.contentY,64);checks++;
// Text entry, details and confirmations no longer block shoulder navigation.
for(const page of ['audio','display','network','bluetooth','controllers','notification-settings','power','reboot','shutdown','exit','store-close','appearance']){
 const t=fixture(page);t.networkPageLoader.item={editingPassword:true};t.bluetoothPageLoader.item={editingCode:true};
 t.switchTab(1);assert.equal(t.activeTab,1);t.switchTab(-1);assert.equal(t.page,page);checks++;
}
const confirmation=fixture('notifications');confirmation.activeTab=1;
confirmation.notificationsPage.blocked=true;confirmation.notificationsPage.confirming=true;
confirmation.switchTab(1);assert.equal(confirmation.activeTab,2);
confirmation.switchTab(-1);assert.equal(confirmation.page,'notifications');assert.equal(confirmation.notificationsPage.confirming,true);checks++;
const busy=fixture('audio');busy.exitBusy=true;busy.switchTab(1);assert.equal(busy.page,'audio');checks++;
const destructive=fixture('shutdown');destructive.returnPage='power';destructive.hideMenu();destructive.resetMenuSession();
assert.equal(destructive.page,'power','closing cancels destructive confirmation while retaining its parent section');checks++;
// Deferred scroll restoration cannot target a different section after rapid switches.
const rapid=fixture();const deferred=[];rapid.Qt.callLater=fn=>deferred.push(fn);
rapid.menuList.contentY=64;rapid.switchTab(1);rapid.switchTab(1);rapid.switchTab(1);
rapid.page='power';rapid.menuList.contentY=111;deferred.forEach(fn=>fn());assert.equal(rapid.menuList.contentY,111);checks++;
// Exercise actual AudioPage location methods, including disconnected candidates.
const audioSource=fs.readFileSync(path.join(__dirname,'../qml/AudioPage.qml'),'utf8');
const methods=audioSource.match(/^    function \w+\([^\n]*\) \{[^\n]*\}$|^    function \w+\([^\n]*\) \{[\s\S]*?^    \}/gm);
const output={name:'HDMI'};const audio={selected:2,candidateNode:output,outputs:[output],rows:[{},{},{},{}],
 controls:{contentY:44,forceLayout:()=>{}},horizontalDelay:{stop:()=>{}},horizontalStep:{stop:()=>{}},heldHorizontal:1};
vm.createContext(audio);vm.runInContext(methods.join('\n'),audio);const location=audio.location();
audio.selected=0;audio.controls.contentY=0;audio.restoreLocation(location);
assert.equal(audio.selected,2);assert.equal(audio.controls.contentY,44);assert.equal(audio.heldHorizontal,0);checks++;
audio.outputs=[];audio.candidateNode=null;audio.restoreLocation(location);assert.equal(audio.candidateNode,null);checks++;
// Success color depends on typed state, not a translated title; warning wins.
const card=fs.readFileSync(path.join(__dirname,'../qml/NotificationToastCard.qml'),'utf8');
const match=card.match(/readonly property bool connectedSuccess: ([^\n]+)\n( +&&[^\n]+)/);
const connected=match[1]+match[2];
const accent=card.match(/readonly property string accent: ([^\n]+)/)[1];
for(const [category,state,urgency,expected] of [['input','connected',1,'#8AB9A1'],['input','disconnected',1,'#FF9A90'],['input','connected',2,'#FF9A90'],['input','',1,'#EAC28D'],['network','connected',1,'#8AB9A1'],['network','error',1,'#FF9A90'],['bluetooth','enabled',1,'#8AB9A1'],['bluetooth','disabled',1,'#A4B5C8'],['install','installed',1,'#8AB9A1'],['install','removed',1,'#EAC28D'],['install','error',1,'#FF9A90']]){
 const record={category,state,urgency};const connectedSuccess=vm.runInNewContext(connected,{record}); const installSuccess=record.category === "install" && record.state === "installed";
 assert.equal(vm.runInNewContext(accent,{record,connectedSuccess,installSuccess}),expected);checks++;
}
// The global surface yields to the menu's shared stack instead of being covered.
const renderer=fs.readFileSync(path.join(__dirname,'../notification-shell.qml'),'utf8');
assert.match(renderer,/!notifications.menuOpen && stack.hasCards/);
assert.match(renderer,/NotificationToastStack/);assert.match(source,/NotificationToastStack/);
assert.match(source,/presentingMenu: root.menuPresentsToasts/);
const presentation=source.match(/readonly property bool menuPresentsToasts: ([^\n]+)\n( +&&[^\n]+)/);
for(const [screen,present,expected] of [['HDMI-A-1',true,true],['DP-1',true,false],['HDMI-A-1',false,false]]){
 const visible=vm.runInNewContext(presentation[1]+presentation[2],{menuOpen:true,notifications:{available:true,outputPresent:present,outputName:'HDMI-A-1'},overlay:{screen:{name:screen}}});
 assert.equal(visible,expected);checks++;
}
assert.doesNotMatch(source,/BACK TO SWITCH TABS/);
const clientSource=fs.readFileSync(path.join(__dirname,'../qml/NotificationClient.qml'),'utf8');
const receive=clientSource.match(/^    function receive\(line\) \{[\s\S]*?^    \}/m)[0];
const sent=[];const client={available:false,viewing:true,presentingMenu:true,send:message=>sent.push(message)};
vm.createContext(client);vm.runInContext(receive,client);
client.receive(JSON.stringify({type:'notifications',records:[],visible:[{id:'current'}],menuOpen:true}));
assert.equal(client.popups[0].id,'current');assert.equal(client.menuOpen,true);
assert.deepEqual(Array.from(sent,m=>m.action),['view','menu']);checks++;
client.receive(JSON.stringify({type:'notifications',visible:[],menuOpen:false}));
assert.equal(sent.length,2,'state updates do not resend presentation commands');assert.equal(client.menuOpen,false);checks++;
console.log(`PASS: ${checks} remembered sections, safe tab switches, reopen/scroll races and toast presentation/color cases.`);
