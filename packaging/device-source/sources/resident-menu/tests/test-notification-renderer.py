import ast
import asyncio
from pathlib import Path
import re
import threading
import types
import unittest

ROOT=Path(__file__).resolve().parents[1]

class RendererTests(unittest.TestCase):
    def test_all_toast_relative_imports_stay_inside_config_root(self):
        entry=ROOT/'notification-shell.qml'
        self.assertTrue(entry.is_file())
        for path in [entry,ROOT/'notifications/ToastCard.qml',ROOT/'qml/NotificationClient.qml']:
            for relative in re.findall(r'^import\s+"([^"]+)"',path.read_text(),re.M):
                target=(path.parent/relative).resolve()
                self.assertTrue(target.is_relative_to(entry.parent.resolve()),str(target))
                self.assertTrue(target.exists(),str(target))
        menu_root=ROOT/'qml'
        for path in [menu_root/'NotificationToastStack.qml',menu_root/'NotificationToastCard.qml']:
            for relative in re.findall(r'^import\s+"([^"]+)"',path.read_text(),re.M):
                self.assertTrue((path.parent/relative).resolve().is_relative_to(menu_root.resolve()))
    def fixture(self):
        tree=ast.parse((ROOT/'notifications/server.py').read_text())
        node=next(n for n in ast.walk(tree) if isinstance(n,ast.AsyncFunctionDef) and n.name=='dispatch')
        status={}
        scope={'lock':threading.RLock(),'peers':{'watched':object()},'renderer_status':status,
            'model':types.SimpleNamespace(clock=lambda:1234, command=lambda message,peer:{'type':'notifications','records':[]}),
            'snapshot':lambda:{'renderer':dict(status)},'GLib':types.SimpleNamespace(idle_add=lambda callback:None)}
        exec(compile(ast.Module(body=[node],type_ignores=[]),'<actual server dispatch>','exec'),scope)
        return scope,status
    def test_heartbeat_uses_direct_status_without_changing_history(self):
        scope,status=self.fixture()
        result=asyncio.run(scope['dispatch']({'action':'renderer-status','output':'HDMI-A-1','outputPresent':True},'watched'))
        self.assertTrue(result['renderer']['outputPresent'])
        self.assertEqual(result['renderer']['updatedAt'],1234)
        self.assertEqual(status['peer'],'watched')
    def test_output_presence_change_is_published_once_without_heartbeat_churn(self):
        scope,status=self.fixture();scheduled=[]
        scope['GLib']=types.SimpleNamespace(idle_add=lambda callback:scheduled.append(callback))
        for present in [True,True,True,False,False,True]:
            asyncio.run(scope['dispatch']({'action':'renderer-status','output':'HDMI-A-1','outputPresent':present},'watched'))
        self.assertEqual(len(scheduled),3)
    def test_direct_status_preserves_received_renderer_heartbeat(self):
        scope,status=self.fixture()
        asyncio.run(scope['dispatch']({'action':'renderer-status','output':'HDMI-A-1','outputPresent':True},'watched'))
        # The one-shot diagnostics path must return the envelope, not just Model.snapshot().
        scope['GLib']=types.SimpleNamespace(idle_add=lambda callback:None)
        result=asyncio.run(scope['dispatch']({'action':'snapshot'},'status-client'))
        self.assertTrue(result['renderer']['outputPresent'])
        self.assertEqual(result['renderer']['updatedAt'],1234)
    def test_one_shot_request_cannot_spoof_renderer_readiness(self):
        scope,status=self.fixture()
        with self.assertRaisesRegex(ValueError,'watched connection'):
            asyncio.run(scope['dispatch']({'action':'renderer-status','outputPresent':True},'one-shot'))
        self.assertEqual(status,{})
    def test_menu_connection_loss_releases_standalone_presentation(self):
        tree=ast.parse((ROOT/'notifications/server.py').read_text())
        peer=next(n for n in ast.walk(tree) if isinstance(n,ast.AsyncFunctionDef) and n.name=='peer')
        cleanup=next(n for n in peer.body if isinstance(n,ast.Try)).finalbody
        node=ast.AsyncFunctionDef(name='cleanup',args=ast.arguments(posonlyargs=[],args=[],kwonlyargs=[],kw_defaults=[],defaults=[]),body=cleanup,decorator_list=[])
        module=ast.fix_missing_locations(ast.Module(body=[node],type_ignores=[]))
        published=[]
        async def closed():pass
        scope={'identity':'resident','peers':{'resident':object()},'renderer_status':{},
            'lock':threading.RLock(),'model':types.SimpleNamespace(viewers={'resident'},presenters={'resident'}),
            'writer':types.SimpleNamespace(close=lambda:None,wait_closed=closed),'publish':lambda:published.append(True)}
        exec(compile(module,'<actual peer cleanup>','exec'),scope)
        asyncio.run(scope['cleanup']())
        self.assertEqual(scope['model'].presenters,set())
        self.assertEqual(scope['model'].viewers,set())
        self.assertEqual(published,[True])

if __name__=='__main__':unittest.main()
