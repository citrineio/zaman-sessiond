"""Real subprocess/PTY tests without network, provider credentials or Qt."""
import json
from pathlib import Path
import sys
import threading
import time
import unittest
from unittest.mock import patch
from unittest.mock import MagicMock
import io

sys.path.insert(0,str(Path(__file__).resolve().parents[1]/"receiver"))
from zaman_scraper import Scraper, metadata, question
from test_transfer import Setup

FAKE = r'''#!/usr/bin/env python3
import sys
from pathlib import Path
a=sys.argv[1:]
def arg(k):return a[a.index(k)+1]
root=Path(arg('-i'));out=Path(arg('-g'))/'metadata.pegasus.txt'
if '-s' in a:
 if 'interactive' in a:
  print("Potential entries for 'A game':",flush=True)
  print("1 : Title:    'First' <-- Skyscraper's choice\n    Platform: 'Test'\n    Release date: '2000'",flush=True)
  print("2 : Title:    'Second'\n    Platform: 'Test'\n    Release date: '2001'",flush=True)
  print("-1: NONE OF THE ABOVE!",flush=True)
  print("Please choose the preferred entry (Or press enter to pick Skyscraper's choice): ",end='',flush=True)
  answer=input()
  if answer not in ('','1','2','-1'):sys.exit(9)
 print('(1/1) Gathered',flush=True)
 sys.exit(0)
if out.exists():
 print("'metadata.pegasus.txt' already exists, do you want to overwrite it (y/N)? ",end='',flush=True)
 if input().lower()!='y':print('User chose not to overwrite, now exiting...');sys.exit(0)
 print('Do you want to skip already existing game list entries (y/N)? ',end='',flush=True)
 input()
media=Path(arg('-o'));media.mkdir(parents=True,exist_ok=True)
lines=['collection: test','shortname: test']
for f in sorted(root.rglob('*')):
 if f.suffix.lower() not in ('.nes','.iso','.cue','.bin'):continue
 art=media/(str(len(lines))+'.png');art.write_bytes(b'art')
 lines+=['','game: Scraped '+f.stem,'file: '+str(f.resolve()),'description: A description','assets.boxFront: '+str(art.resolve())]
out.write_text('\n'.join(lines)+'\n')
print('(1/1) Exported',flush=True)
'''


class ScrapeTests(Setup):
    def setUp(self):
        super().setUp()
        self.binary=self.root/"Skyscraper"
        self.binary.write_text(FAKE);self.binary.chmod(0o755)
        self.program=dict(binary=str(self.binary),platforms=["nes","ps2","psx"],version="3.21.0")
        self.safe=True
        self.scraper=Scraper(self.inbox,lambda:self.safe,program=self.program)
        self.receiver.scraper=self.scraper
        self.addCleanup(self.scraper.close)

    def wait(self, predicate):
        deadline=time.monotonic()+7
        while time.monotonic()<deadline:
            if predicate():return
            time.sleep(.02)
        self.fail('Scraping condition timed out: '+str(self.scraper.snapshot()))

    def answer_all(self, ident):
        job=self.scraper.jobs[ident]
        deadline=time.monotonic()+7
        while job['phase'] not in {'complete','failed','cancelled','interrupted'}:
            if time.monotonic()>deadline:self.fail(str(job))
            prompt=job.get('prompt')
            if prompt:
                value='y' if 'overwrite' in prompt['question'] else 'n' if '(y/N)' in prompt['question'] else '2'
                self.scraper.answer(ident,prompt['id'],value)
            time.sleep(.02)
        return job

    def test_collection_discovery_and_unsupported_folders(self):
        for name in ['ps2','others','nonsense','gameboy']:(self.rom/name).mkdir()
        (self.rom/'linked').symlink_to(self.rom/'ps2',target_is_directory=True)
        self.scraper.scan()
        rows={f['id']:f for f in self.scraper.snapshot()['folders']}
        self.assertTrue(rows['ps2']['supported'])
        self.assertFalse(rows['others']['supported'])
        self.assertFalse(rows['nonsense']['supported'])
        self.assertFalse(rows['linked']['supported'])
        with self.assertRaises(ValueError):self.scraper.start('../ps2')

    def test_import_automatically_gets_artwork_then_publishes_without_a_decision(self):
        self.inbox.require_scrape=True
        ident=self.deliver({'Game.nes':b'NESdata'})['Game.nes']
        self.assertEqual(self.inbox.get(ident)['phase'],'needs-scrape')
        self.receiver.work_once(safe=lambda:True)
        self.assertFalse((self.rom/'nes').exists())
        self.scraper.begin()
        jobid=self.inbox.get(ident)['scrape_job']
        self.assertTrue(self.scraper.jobs[jobid]['automatic'])
        self.assertFalse(self.scraper.jobs[jobid]['review'])
        self.wait(lambda:self.scraper.jobs[jobid]['phase']=='complete')
        job=self.scraper.jobs[jobid]
        self.assertEqual(job['phase'],'complete',job)
        self.assertEqual(self.inbox.get(ident)['phase'],'ready')
        self.assertFalse((self.rom/'nes').exists())
        self.receiver.work_once(safe=lambda:True)
        self.assertEqual(self.inbox.get(ident)['phase'],'complete')
        self.assertEqual(self.inbox.get(ident)['artwork_warning'],'')
        text=(self.root/'metafiles/zaman-transfer-nes.metadata.pegasus.txt').read_text()
        self.assertIn('game: Scraped Game',text)
        self.assertIn('description: A description',text)
        self.assertIn('scrape-media',text)
        self.assertNotIn('/work/',text)
        self.assertEqual(list((self.scraper.root/jobid/'work').glob('*/input')),[])

    def test_skip_scraping_publishes_and_later_manual_export_enriches_without_duplicates(self):
        self.inbox.require_scrape=True
        ident=self.deliver({'Imported.nes':b'NESdata'})['Imported.nes']
        self.receiver.control(dict(action='import-scrape',id=ident,decision='skip'))
        self.inbox.publish(ident)
        folder=self.rom/'nes'
        (folder/'Old.nes').write_bytes(b'old')
        meta=folder/'metadata.pegasus.txt'
        meta.write_text('collection: Nintendo Entertainment System\nshortname: nes\nlaunch: /usr/local/bin/zamanctl launch nes "{file.path}"\n\ngame: Old\nfile: '+str(folder/'Old.nes')+'\n')
        self.scraper.scan();self.scraper.begin()
        jobid=self.scraper.start('nes','offline')
        job=self.answer_all(jobid)
        self.assertEqual(job['phase'],'complete',job)
        self.assertIn('collection: Nintendo Entertainment System',meta.read_text())
        self.assertNotIn('Imported.nes',meta.read_text())
        self.assertIn('Old.nes',meta.read_text())
        owned=self.root/'metafiles/zaman-transfer-nes.metadata.pegasus.txt'
        self.assertIn('game: Scraped Imported',owned.read_text())
        self.assertTrue(self.scraper.needs_refresh())
        self.inbox.write_metadata()
        self.assertIn('game: Scraped Imported',owned.read_text())
        self.assertTrue((self.rom/'nes'/Path(self.inbox.get(ident)['target']).name/'Imported.nes').exists())

    def test_default_preserves_existing_fields_and_answers_private_file_questions(self):
        folder=self.rom/'nes';folder.mkdir();(folder/'Old.nes').write_bytes(b'old')
        meta=folder/'metadata.pegasus.txt';meta.write_text('collection: Existing\ngame: Old\nfile: Old.nes\n')
        self.scraper.scan();self.scraper.begin();ident=self.scraper.start('nes','offline')
        self.wait(lambda:self.scraper.jobs[ident]['phase']=='complete')
        self.assertIsNone(self.scraper.jobs[ident]['prompt'])
        header,entries=metadata(meta.read_text())
        self.assertEqual(header['collection'],'Existing')
        self.assertEqual(entries[0]['game'],'Old')
        self.assertIn('assets.boxFront',entries[0])
        self.assertEqual((folder/'Old.nes').read_bytes(),b'old')

    def test_waits_through_gameplay_and_cancel_releases_import(self):
        self.inbox.require_scrape=True
        ident=self.deliver({'A.nes':b'NES'})['A.nes']
        self.safe=False;self.scraper.begin()
        jobid=self.scraper.start(None,ident=ident)
        self.wait(lambda:self.scraper.jobs[jobid]['phase']=='waiting-library')
        self.assertFalse((self.scraper.root/jobid/'work').exists())
        self.scraper.cancel(jobid)
        self.assertEqual(self.inbox.get(ident)['phase'],'needs-scrape')
        self.assertTrue(self.inbox.path(ident).exists())

    def test_interrupted_job_persists_and_pending_import_returns_to_decision(self):
        self.inbox.require_scrape=True
        ident=self.deliver({'A.nes':b'NES'})['A.nes']
        jobid=self.scraper.start(None,ident=ident)
        restored=Scraper(self.inbox,lambda:True,program=self.program)
        try:
            self.assertEqual(restored.jobs[jobid]['phase'],'interrupted')
            self.assertEqual(self.inbox.get(ident)['phase'],'needs-scrape')
        finally:restored.close()

    def test_running_question_pauses_during_gameplay_and_resumes(self):
        self.inbox.require_scrape=True
        ident=self.deliver({'A.nes':b'NES'})['A.nes']
        self.scraper.begin();jobid=self.scraper.start(None,review=True,ident=ident)
        self.wait(lambda:self.scraper.jobs[jobid]['prompt'] is not None)
        prompt=self.scraper.jobs[jobid]['prompt']
        self.safe=False
        self.wait(lambda:self.scraper.jobs[jobid]['phase']=='waiting-library')
        self.assertEqual(self.scraper.jobs[jobid]['prompt']['id'],prompt['id'])
        self.safe=True
        self.wait(lambda:self.scraper.jobs[jobid]['phase']=='needs-input')
        self.scraper.answer(jobid,prompt['id'],'-1')
        self.wait(lambda:self.scraper.jobs[jobid]['phase']=='complete')
        self.assertEqual(self.inbox.get(ident)['phase'],'ready')

    def test_manual_export_refuses_metadata_changed_while_question_pending(self):
        folder=self.rom/'nes';folder.mkdir();(folder/'Old.nes').write_bytes(b'old')
        meta=folder/'metadata.pegasus.txt';meta.write_text('collection: Existing\ngame: Old\nfile: Old.nes\n')
        self.scraper.scan();self.scraper.begin();ident=self.scraper.start('nes','cached',True)
        self.wait(lambda:self.scraper.jobs[ident]['prompt'] is not None)
        newer='collection: A newer user edit\ngame: Old\nfile: Old.nes\n'
        meta.write_text(newer)
        job=self.answer_all(ident)
        self.assertEqual(job['phase'],'failed',job)
        self.assertIn('changed during scraping',job['results'][0]['error'])
        self.assertEqual(meta.read_text(),newer)
        self.assertEqual((folder/'Old.nes').read_bytes(),b'old')

    def automatic_failure(self, source):
        self.binary.write_text(source)
        self.inbox.require_scrape=True
        ident=self.deliver({'A.nes':b'NES'})['A.nes']
        self.receiver.work_once(safe=lambda:True);self.scraper.begin()
        self.wait(lambda:self.inbox.get(ident)['phase']=='ready')
        row=self.inbox.get(ident);self.assertTrue(row['artwork_warning'])
        self.assertEqual(row['error'],'')
        self.receiver.work_once(safe=lambda:True)
        row=self.inbox.get(ident);self.assertEqual(row['phase'],'complete')
        self.assertTrue((Path(row['target'])/row['name']).is_file())
        return ident

    def test_no_internet_no_cache_still_imports_and_exposes_manual_recovery(self):
        source=FAKE.replace("print('(1/1) Gathered',flush=True)","print('Network unreachable',flush=True);sys.exit(2)")
        source=source.replace(",'description: A description','assets.boxFront: '+str(art.resolve())",'')
        ident=self.automatic_failure(source)
        self.assertIn('Check your connection',self.inbox.get(ident)['artwork_warning'])
        jobid=self.inbox.get(ident)['scrape_job'];self.binary.write_text(FAKE)
        self.scraper.scan();retry=self.scraper.retry(jobid)
        self.assertEqual(self.scraper.jobs[retry]['imports'],[],'Retry after import must use the published folder')
        self.wait(lambda:self.scraper.jobs[retry]['phase']=='complete')
        self.assertEqual(self.inbox.get(ident)['artwork_warning'],'')

    def test_unmatched_fallback_export_does_not_claim_artwork_success(self):
        source=FAKE.replace(",'description: A description','assets.boxFront: '+str(art.resolve())",'')
        self.automatic_failure(source)

    def test_network_error_can_still_use_existing_cached_output(self):
        source=FAKE.replace("print('(1/1) Gathered',flush=True)","print('Network unreachable',flush=True);sys.exit(2)")
        self.binary.write_text(source);self.inbox.require_scrape=True
        ident=self.deliver({'Cached.nes':b'NES'})['Cached.nes']
        self.receiver.work_once(safe=lambda:True);self.scraper.begin()
        self.wait(lambda:self.inbox.get(ident)['phase']=='ready')
        self.assertEqual(self.inbox.get(ident)['artwork_warning'],'')
        self.assertIn('assets.boxFront',json.loads(self.inbox.get(ident)['scraped']))

    def test_default_fills_broken_artwork_but_refresh_replaces_user_fields(self):
        folder=self.rom/'nes';folder.mkdir();(folder/'Old.nes').write_bytes(b'old')
        meta=folder/'metadata.pegasus.txt'
        meta.write_text('collection: Existing\ngame: My title\nfile: Old.nes\ndescription: My description\nassets.boxFront: missing.png\nx-custom: keep\n')
        self.scraper.scan();self.scraper.begin();ident=self.scraper.start('nes','offline')
        self.wait(lambda:self.scraper.jobs[ident]['phase']=='complete')
        entry=metadata(meta.read_text())[1][0]
        self.assertEqual(entry['game'],'My title');self.assertEqual(entry['description'],'My description')
        self.assertTrue(Path(entry['assets.boxFront']).is_file())
        ident=self.scraper.start('nes','refresh')
        self.wait(lambda:self.scraper.jobs[ident]['phase']=='complete')
        entry=metadata(meta.read_text())[1][0]
        self.assertEqual(entry['game'],'Scraped Old');self.assertEqual(entry['x-custom'],'keep')

    def test_automatic_question_never_blocks_game_import(self):
        source=FAKE.replace("if '-s' in a:","if '-s' in a:\n print('Account password?',end='',flush=True);input()")
        source=source.replace(",'description: A description','assets.boxFront: '+str(art.resolve())",'')
        self.automatic_failure(source)

    def test_automatic_timeout_releases_the_game(self):
        with patch('zaman_scraper.AUTO_BUDGET_SECONDS',.01):
            self.automatic_failure(FAKE)

    def test_unavailable_scraper_and_busy_manual_job_do_not_block_imports(self):
        self.inbox.require_scrape=True;self.scraper.available=False
        ident=self.deliver({'Unavailable.nes':b'NESone'})['Unavailable.nes']
        self.receiver.work_once(safe=lambda:True)
        self.assertEqual(self.inbox.get(ident)['phase'],'complete')
        self.assertTrue(self.inbox.get(ident)['artwork_warning'])
        self.scraper.available=True;self.scraper.scan();self.scraper.start('nes')
        second=self.deliver({'Busy.nes':b'NEStwo'})['Busy.nes']
        self.receiver.work_once(safe=lambda:True)
        self.assertEqual(self.inbox.get(second)['phase'],'complete')

    def test_multiple_transfers_queue_automatically_and_do_not_duplicate_jobs(self):
        self.inbox.require_scrape=True
        ids=self.deliver({'One.nes':b'NESone','Two.nes':b'NEStwo'})
        self.receiver.work_once(safe=lambda:True);self.receiver.work_once(safe=lambda:True)
        self.assertEqual(len(self.scraper.jobs),2)
        self.scraper.begin();self.wait(lambda:all(self.inbox.get(ident)['phase']=='ready' for ident in ids.values()))
        self.receiver.work_once(safe=lambda:True)
        self.assertTrue(all(self.inbox.get(ident)['phase']=='complete' for ident in ids.values()))

    def test_ambiguous_iso_waits_only_for_collection_then_looks_up_automatically(self):
        self.inbox.require_scrape=True
        ident=self.deliver({'Disc.iso':b'disc'})['Disc.iso']
        self.receiver.work_once(safe=lambda:True)
        self.assertEqual(self.inbox.get(ident)['phase'],'needs-destination');self.assertEqual(len(self.scraper.jobs),0)
        self.receiver.control(dict(action='choose',id=ident,system='ps2'))
        self.receiver.work_once(safe=lambda:True);self.scraper.begin()
        self.wait(lambda:self.inbox.get(ident)['phase']=='ready')
        self.receiver.work_once(safe=lambda:True)
        self.assertEqual(self.inbox.get(ident)['phase'],'complete')
        self.assertEqual(self.inbox.get(ident)['artwork_warning'],'')

    def test_automatic_restart_releases_import_instead_of_repeating_lookup(self):
        self.inbox.require_scrape=True
        ident=self.deliver({'A.nes':b'NES'})['A.nes'];self.scraper.auto_import(ident)
        restored=Scraper(self.inbox,lambda:True,program=self.program)
        try:
            self.assertEqual(self.inbox.get(ident)['phase'],'ready')
            self.assertTrue(self.inbox.get(ident)['artwork_warning'])
        finally:restored.close()

    def test_prompt_parser_and_metadata_multiline(self):
        prompt=question('\033[1;34mOverwrite this (y/N)? \033[0m')
        self.assertEqual([c['value'] for c in prompt['choices']],['n','y'])
        self.assertIsNone(question('Downloading assets...'))
        self.assertTrue(question('Unexpected question?')['text'])
        header,entries=metadata('collection: A\ngame: B\nfile: B.nes\ndescription: First\n  Second\n')
        self.assertEqual(entries[0]['description'],'First\nSecond')


class TransferStageTests(Setup):
    def setUp(self):
        super().setUp()
        self.theme=self.root/'theme';self.theme.mkdir();(self.theme/'theme.qml').write_text('test theme')
        self.receiver.theme=self.theme
        self.context={"Status":["Idle","","","","",""],"ForegroundStatus":["menu","library",""],
                      "InstalledGameStatus":["0","","","Exited","",False],
                      "StorefrontStatus":["",0,"","",False,""]}

    def test_menu_holds_ready_and_library_close_publishes_then_notifies(self):
        from zaman_receiver import safe_to_import, safe_to_publish
        ident=self.deliver({'Stage.nes':b'NESdata'})['Stage.nes']
        seen=[]
        def notice(row):
            seen.append(row['phase'])
            if row['phase']=='complete':
                self.assertTrue((self.root/'metafiles/zaman-transfer-nes.metadata.pegasus.txt').is_file())
                self.assertEqual(len(list((self.theme/'rom-library-refresh').glob('*.signal'))),1)
            return True
        with patch('zaman_receiver.bus_call',side_effect=lambda method:self.context[method]),patch('zaman_receiver.notify',side_effect=notice):
            self.assertTrue(safe_to_publish(),'Artwork can finish while the menu is open')
            self.assertFalse(safe_to_import())
            self.receiver.work_once();self.receiver.work_once()
            self.assertEqual(seen,['ready'],'Polling does not repeat the readiness toast')
            self.assertEqual(self.inbox.get(ident)['phase'],'ready')
            self.assertFalse((self.rom/'nes').exists())
            self.context['ForegroundStatus']=['library','','home']
            self.receiver.work_once();self.receiver.work_once()
            self.assertEqual(seen,['ready','complete'])
            self.assertEqual(self.inbox.get(ident)['phase'],'complete')

    def test_import_gate_stays_closed_for_games_and_bus_failure(self):
        from zaman_receiver import safe_to_import
        with patch('zaman_receiver.bus_call',side_effect=lambda method:self.context[method]):
            self.context['ForegroundStatus']=['library','','']
            self.assertTrue(safe_to_import())
            self.context['Status'][0]='Running'
            self.assertFalse(safe_to_import())
        with patch('zaman_receiver.bus_call',side_effect=OSError('bus lost')):
            self.assertFalse(safe_to_import())

    def test_ready_notice_retries_failure_and_survives_receiver_restart(self):
        from zaman_receiver import Receiver
        ident=self.deliver({'Stage.nes':b'NESdata'})['Stage.nes']
        with patch('zaman_receiver.notify',side_effect=[False,True]) as notice:
            self.receiver.work_once(safe=lambda:False);self.receiver.work_once(safe=lambda:False)
            self.assertEqual(notice.call_count,2)
        restored=Receiver(self.inbox,theme=self.theme)
        with patch('zaman_receiver.notify',return_value=True) as notice:
            restored.work_once(safe=lambda:False);notice.assert_not_called()
        self.assertEqual(self.inbox.get(ident)['notified'],'ready')

    def test_metadata_failure_never_announces_completion_early(self):
        ident=self.deliver({'Stage.nes':b'NESdata'})['Stage.nes']
        original=self.inbox.write_metadata
        calls=0
        def fail_rebuild():
            nonlocal calls
            calls+=1
            if calls>2:raise OSError('disk full')
            return original()
        with patch.object(self.inbox,'write_metadata',side_effect=fail_rebuild),patch('zaman_receiver.notify') as notice:
            with self.assertRaisesRegex(OSError,'disk full'):self.receiver.work_once(safe=lambda:True)
            notice.assert_not_called()
        with patch('zaman_receiver.notify',return_value=True) as notice:
            self.receiver.work_once(safe=lambda:False);notice.assert_not_called()
            self.receiver.work_once(safe=lambda:True)
            self.assertEqual(notice.call_count,1)
        self.assertEqual(self.inbox.get(ident)['notified'],'complete')

    def test_refresh_failure_retries_before_completion_notice(self):
        self.deliver({'Stage.nes':b'NESdata'})
        with patch.object(self.receiver,'signal_refresh',side_effect=OSError('refresh failed')),patch('zaman_receiver.notify') as notice:
            with self.assertRaisesRegex(OSError,'refresh failed'):self.receiver.work_once(safe=lambda:True)
            notice.assert_not_called()
        with patch('zaman_receiver.notify',return_value=True) as notice:
            self.receiver.work_once(safe=lambda:False);notice.assert_not_called()
            self.receiver.work_once(safe=lambda:True);self.assertEqual(notice.call_count,1)

    def test_approved_toast_copy_and_supported_route(self):
        from zaman_receiver import notify
        for phase,warning,title in [('ready','','Game ready to add'),('ready','No network','Ready to add without artwork'),
                                    ('complete','','Added to your library'),('complete','No network','Added to your library')]:
            peer=MagicMock();peer.__enter__.return_value=peer
            peer.makefile.return_value=io.BytesIO(b'{"type":"ok"}\n')
            with patch('zaman_receiver.socket.socket',return_value=peer):
                self.assertTrue(notify(dict(id='a'*32,name='Stage.nes',phase=phase,artwork_warning=warning)))
            event=json.loads(peer.sendall.call_args.args[0])
            self.assertEqual(event['title'],title);self.assertEqual(event['target'],'transfer-pending')
            if phase=='ready':self.assertIn('Close the menu to update your library.',event['body'])
            else:self.assertIn('Your game is ready to play.',event['body'])
            if warning and phase=='ready':self.assertIn('You can find artwork later in Artwork & Game Info.',event['body'])


if __name__=='__main__':unittest.main()
