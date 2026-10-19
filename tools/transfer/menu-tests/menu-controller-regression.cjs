// Runs the actual shell.qml JavaScript functions; no Qt/device runtime required.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
const source = fs.readFileSync(path.join(__dirname, '../qml/shell.qml'), 'utf8');
const functions = source.match(/^    function \w+\([^\n]*\) \{[^\n]*\}$|^    function \w+\([^\n]*\) \{[\s\S]*?^    \}/gm);
assert.equal(functions.length, 36, 'all root functions must be exercised from the real source');
let checks = 0;
function fixture(page = '') {
  const state = {root:null, activeTab:0, tabLocations:[null,null,null], navigationRevision:0, audioLoaded:false, networkLoaded:false, bluetoothLoaded:false,
    notifications:{available:true, records:[], popups:[], preferences:{dnd:false,steamReminder:true}, send:msg=>state.actions.push(msg)},
    notificationsPage:{blocked:false, detail:null, confirming:false, move:()=>{}, activate:()=>{}, back:()=>false, dismiss:()=>state.actions.push('dismiss'), clear:()=>state.actions.push('clear')}, storefrontId:"", storeGeneration:"0", storeGame:false, pendingStoreAction:null, storeAction:{running:false}, storeNotice:"", storeResultReceived:false, menuGeneration:0, menuInstance:'test', sessionAction:'exit-game', exitBusy:false, exitResultReceived:false, exitNotice:'', sessionActionPath:'/test/action.py', exitAction:{running:false}, exitPage:{move:()=>{}, activate:()=>{}}, menuOpen:true, opening:false, ready:true, closing:false, helperIdle:false, lastReadyMs:-1, lastVisibleMs:-1, openStartedMs:0, page, selected:0, mainSelected:0,
    entryPhase:'hidden', firstFrameMs:-1, slideFinishedMs:-1,
    panel:{entryProgress:0, width:700, height:1028}, overlay:{width:1920,height:1080,backingWindowVisible:true},
    slideIn:{duration:180,starts:0,stop:()=>{},restart:()=>state.slideIn.starts++},
    slideOut:{starts:0,stop:()=>{},restart:()=>state.slideOut.starts++},
    closeDeadline:{stop:()=>{},restart:()=>{}},closeOrigin:{page:"",tab:0},
    settingsSelected:0, powerSelected:0, pageEntry:{label:'',key:'',detail:''}, returnPage:'settings', pressed:{}, heldDirection:0, heldHorizontal:0, keyboardCycleHeld:false, controllerWarning:'',
    controllerCount:1, controllerDevices:[], foreground:'game', displaySupported:true,
    warnings:[], writes:[], quit:0, actions:[],
    console:{warn:()=>{}, error:()=>{}, info:()=>{}}, Qt:{callLater:fn=>fn(), quit:()=>state.quit++, formatDateTime:()=> '7:00 PM'},
    bridge:{running:true, write:value=>state.writes.push(JSON.parse(value))},
    repeatDelay:{stop:()=>{}, restart:()=>{}}, repeatStep:{stop:()=>{}, start:()=>{}},
    horizontalDelay:{stop:()=>{}, restart:()=>{}}, horizontalStep:{stop:()=>{}, start:()=>{}},
    menuList:{contentY:0, forceLayout:()=>{}, positionViewAtIndex:()=>{}}, ListView:{Contain:0},
    bluetoothPageLoader:{item:null}, networkPageLoader:{item:null}, audioPageLoader:{item:null},
    transactionActivity:{move:()=>{},activate:()=>{}}, transactionJobs:[], transactionsAvailable:true, pendingTransactions:0, pendingImports:0,
    transferPage:{move:()=>{},activate:()=>{},back:()=>false,dismiss:()=>state.actions.push('transfer-dismiss'),clear:()=>state.actions.push('transfer-clear')},
    scrapePage:{editingText:false,open:(...args)=>state.actions.push({scrapeOpen:args}),move:()=>{},horizontal:()=>{},activate:()=>{},back:()=>false,dismiss:()=>state.actions.push('scrape-dismiss'),clear:()=>state.actions.push('scrape-clear')},
    controllersPage:{move:()=>{}, activate:()=>{}},
    displayPage:{receive:()=>state.actions.push('display'), back:()=>state.actions.push('back'),
      activate:()=>{}, horizontal:()=>{}, move:()=>{}},
    mainEntries:[{key:'resume'}, {key:'settings'}], settingsEntries:[{key:'controllers'}]};
  Object.defineProperties(state, {
    canSwitchTab:{get:()=>vm.runInContext(source.match(/readonly property bool canSwitchTab: ([^\n]+)/)[1],state)},
    listPage:{get:()=>['','settings','power','storefronts','notification-settings'].includes(state.page)},
    navigationPage:{get:()=>true},
    editingKeyboard:{get:()=>state.page === 'network' && !!state.networkPageLoader.item?.editingPassword
      || state.page === 'bluetooth' && !!state.bluetoothPageLoader.item?.editingCode || state.page === 'scrape' && state.scrapePage.editingText},
    entries:{get:()=>state.page === 'settings' ? state.settingsEntries : state.mainEntries}
  });
  state.root=state;
  vm.createContext(state); vm.runInContext(functions.join('\n'), state);
  return state;
}
function staysOpen(state, originalPage) {
  assert.equal(state.menuOpen,true); assert.equal(state.closing,false);
  assert.equal(state.quit,0); assert.equal(state.page,originalPage);
  assert.equal(state.writes.filter(x=>x.action === 'close').length,0); checks++;
}
for (const page of ['', 'settings', 'controllers', 'audio', 'network', 'bluetooth', 'display', 'appearance', 'exit', 'reboot', 'shutdown', 'storefronts', 'store-close','notifications','notification-settings','transactions','transfer']) {
  const state = fixture(page);
  state.heldDirection = -1; state.pressed = {ui_up:true};
  for (let i=0; i<20; i++) {
    state.receive(JSON.stringify({type:'input-reset'}));
    state.receive(JSON.stringify({type:'warning', message:'service/ownership/transport/disconnect failure'}));
    state.receive(JSON.stringify({type:'controllers', controllers:0, devices:[]}));
    staysOpen(state,page);
    assert.equal(state.heldDirection,0); assert.equal(Object.keys(state.pressed).length,0);
  }
  for (const event of ['ui_accept','ui_back','ui_cancel','ui_left','ui_right','ui_up','ui_down','ui_guide']) {
    state.input(event,0); staysOpen(state,page);
    for (const value of [NaN, Infinity, -Infinity, '1', null]) {
      state.input(event,value); staysOpen(state,page);
    }
  }
  state.receive(JSON.stringify({type:'controller-recovered'}));
  assert.equal(state.controllerWarning,''); staysOpen(state,page);
}
// Reconnection retains the current page and restores usable navigation.
const recovered = fixture();
recovered.receive(JSON.stringify({type:'input-reset'}));
recovered.receive(JSON.stringify({type:'controllers', controllers:1, devices:[{id:'new'}]}));
recovered.input('ui_down',1); assert.equal(recovered.selected,1); staysOpen(recovered,'');
// Deliberate user close actions are explicitly preserved.
for (const action of [state=>state.input('ui_back',1), state=>state.input('ui_accept',1), state=>state.back()]) {
  const state = fixture(); action(state);
  assert.equal(state.menuOpen,false); assert.equal(state.closing,true);
  assert.equal(state.writes.filter(x=>x.action === 'close').length,1); checks++;
}
console.log(`PASS: ${checks} menu lifecycle/navigation assertions using actual QML functions.`);
module.exports = {fixture, source};
