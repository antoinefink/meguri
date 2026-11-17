# SPDX-License-Identifier: AGPL-3.0-or-later
"""Finite harness checks; synthetic upstream replies grant no case credit."""
import contextlib
import copy
import http.client
import http.server
import io
import pathlib
import tempfile
import threading
import unittest
from unittest.mock import patch

import adapter as api
import retained_companion as companion
import retained_observer as observer
from validation import Artifacts, decode, encoded, sha


class Upstream(http.server.BaseHTTPRequestHandler):
    protocol_version='HTTP/1.1'
    def log_message(self,*_):pass
    def do_POST(self):
        if self.headers.get('Transfer-Encoding') == 'chunked':
            size=int(self.rfile.readline().strip(),16);data=self.rfile.read(size)
            self.rfile.read(2);self.rfile.readline();self.rfile.read(2)
        else:data=self.rfile.read(int(self.headers.get('Content-Length','0')))
        self.server.requests.append((self.command,self.path,data))
        body=observer.zstandard.ZstdCompressor().compress(self.server.ack) if self.path.startswith('/sync/upload') else b'synthetic-private-response-do-not-retain'
        if self.server.chunk_failure is not None:
            self.send_response(200);self.send_header('Transfer-Encoding','chunked');self.end_headers()
            self.wfile.write(hex(len(body))[2:].encode()+b'\r\n'+body+b'\r\n')
            self.wfile.write(b'5\r\nab' if self.server.chunk_failure == 'truncated' else b'not-a-size\r\n')
            self.wfile.flush();self.close_connection=True
        elif self.server.truncated:
            self.send_response(200);self.send_header('Content-Length',str(len(body)+11));self.end_headers()
            self.wfile.write(body);self.wfile.flush();self.close_connection=True
        else:
            self.send_response(self.server.status);self.send_header('Transfer-Encoding','chunked');self.end_headers()
            self.wfile.write(hex(len(body))[2:].encode()+b'\r\n'+body+b'\r\n0\r\n\r\n');self.wfile.flush()


@contextlib.contextmanager
def upstream(ack=b'OK',truncated=False,status=200,chunk_failure=None):
    server=http.server.ThreadingHTTPServer(observer.TARGET,Upstream)
    server.ack=ack;server.truncated=truncated;server.status=status;server.chunk_failure=chunk_failure;server.requests=[]
    thread=threading.Thread(target=server.serve_forever);thread.start()
    try:yield server
    finally:server.shutdown();server.server_close();thread.join()


class RetainedHarness(unittest.TestCase):
    def test_fixed_actions_identity_and_state_cannot_cross_cases_or_runs(self):
        self.assertEqual(companion.actor_for('normal-bidirectional','prepare_reverse'),'client_b')
        self.assertEqual(companion.actor_for('full-download','witness_download'),'fresh_client')
        for case,action in [('authentication','prepare_rich'),('full-upload','send_reverse'),('unknown','upload_baseline')]:
            with self.assertRaises(ValueError):companion.actor_for(case,action)
        expect={'recipe_sha256':'a'*64,'selection_sha256':'b'*64,'review_sha256':'c'*64}
        invocation={'run_id':'unit-run','source_commit':'0'*40}
        with tempfile.TemporaryDirectory() as directory:
            state=pathlib.Path(directory)
            companion.owner(state,'normal-bidirectional',invocation,expect,new=True)
            companion.owner(state,'normal-bidirectional',invocation,expect)
            for case,current in [('full-download',invocation),('normal-bidirectional',{**invocation,'run_id':'other'})]:
                with self.assertRaises(ValueError):companion.owner(state,case,current,expect)
            with self.assertRaises(ValueError):companion.owner(state,'normal-bidirectional',invocation,expect,new=True)

    def test_proxy_rejects_foreign_targets_unknown_routes_and_offline_actions(self):
        for method,target in [('CONNECT','127.0.0.1:8080'),('POST','http://elsewhere.invalid:8080/sync/meta'),
                              ('POST','http://127.0.0.1:8080/private?secret=do-not-record'),('GET','/sync/meta')]:
            with self.assertRaises(ValueError):observer.route(method,target)
        self.assertNotIn('prepare_initial',observer.WORKER_ACTIONS['authentication'])
        self.assertNotIn('prepare_forward',observer.WORKER_ACTIONS['normal-bidirectional'])
        self.assertEqual(observer.route('POST','http://127.0.0.1:8080/sync/meta?k=secret'),('/sync/meta','/sync/meta?k=secret'))

    def test_worker_admission_binds_exact_preflight_and_never_opens_extra_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root=pathlib.Path(directory);store=Artifacts(root/'artifacts')
            opaque=store.put(b'public-build-input')
            preflight={'server':{key:'a'*64 for key in ('source_commit','source_sha256','lock_sha256','executable_sha256')}}
            raw=encoded(preflight)+b'\n';(root/'preflight.json').write_bytes(raw)
            action={'case':'authentication','action':'authenticate'}
            invocation={'format':'meguri-retained-invocation-v1','run_id':'unit','root':'/public',
                'server_arguments':[],'data_volume':'unit','owner_nonce':'a'*32,**action,'actor':'client_a',
                'client_build_sha256':opaque,'input_manifest_sha256':opaque,'preflight_peer_sha256':sha(raw)}
            for name in ('action.json','invocation.json','client-build.json','input-manifest.json'):(root/name).write_bytes(b'not read by this helper')
            opened=[];original=observer.public_file
            def read(path,*args):opened.append(path.name);return original(path,*args)
            with patch('candidate.retained_dependencies',return_value=set()),patch.dict('os.environ',MG_RUN_ID='unit'),patch.object(observer,'public_file',side_effect=read):
                observer.verify_worker_input(root,action,invocation)
                self.assertEqual(opened,['preflight.json'])
                (root/'preflight.json').write_bytes(encoded(preflight))
                with self.assertRaisesRegex(ValueError,'preflight bytes changed'):observer.verify_worker_input(root,action,invocation)

    def test_actual_deframed_body_counts_exclude_queries_headers_and_body_content(self):
        secret=b'synthetic-private-request-do-not-retain'
        with upstream() as actual:
            with observer.observing('observer-unit','forward','client_a','a'*64) as wire:
                connection=http.client.HTTPConnection(*observer.LISTENER,timeout=5)
                connection.request('POST','http://127.0.0.1:8080/sync/meta?key=synthetic-query-secret',body=iter([secret]),
                    headers={'anki-sync':'synthetic-header-secret'},encode_chunked=True)
                body=connection.getresponse().read();connection.close()
            value=wire.wire();data=encoded(value)
        self.assertEqual(len(actual.requests),1)
        self.assertEqual(value['requests'][0],{'sequence':1,'method':'POST','path':'/sync/meta',
            'request_body_bytes':len(secret),'upstream_status':200,'upstream_body_bytes':len(body),
            'delivered_body_bytes':len(body),'disposition':'forwarded'})
        self.assertEqual(value['totals'],{'request_count':1,'upstream_completed_count':1,'delivered_completed_count':1,
            'request_body_bytes':len(secret),'upstream_body_bytes':len(body),'delivered_body_bytes':len(body),'dropped_body_bytes':0})
        for private in (secret,body,b'synthetic-query-secret',b'synthetic-header-secret',sha(secret).encode(),sha(body).encode()):self.assertNotIn(private,data)
        self.assertFalse(value['case_credit'])

    def test_only_complete_successful_upload_ack_is_dropped_without_proxy_replay(self):
        for ack,status,drop in [(b'not-accepted',200,False),(b'OK',500,False),(b'OK',200,True)]:
            with self.subTest(ack=ack,status=status),upstream(ack=ack,status=status) as actual:
                with observer.observing('observer-unit','uncertain','client_a','a'*64,armed=True) as wire:
                    connection=http.client.HTTPConnection(*observer.LISTENER,timeout=5)
                    connection.request('POST','/sync/upload',body=b'synthetic-collection')
                    if drop:
                        with self.assertRaises(http.client.RemoteDisconnected):connection.getresponse()
                    else:self.assertEqual(connection.getresponse().read(),observer.zstandard.ZstdCompressor().compress(ack))
                    connection.close()
                value=wire.wire()
            self.assertEqual(len(actual.requests),1)
            self.assertEqual(value['requests'][0]['upstream_status'],status)
            self.assertEqual(value['fault'],{'actor':'fault_driver','armed':True,'accepted_upload_ack':drop,
                'reply_dropped':drop,'upload_request_count':1,'upload_replay_count':0})
            self.assertEqual(value['totals']['upstream_completed_count'],1)
            self.assertEqual(value['totals']['delivered_completed_count'],0 if drop else 1)
            self.assertEqual(value['requests'][0]['disposition'],'upload_reply_dropped' if drop else 'forwarded')
            self.assertEqual(value['totals']['dropped_body_bytes'],value['totals']['upstream_body_bytes'] if drop else 0)

    def test_unsupported_method_is_closed_without_logging_url_or_query(self):
        log=io.StringIO()
        with contextlib.redirect_stderr(log),upstream() as actual:
            with observer.observing('observer-unit','invalid','client_a','a'*64) as wire:
                connection=http.client.HTTPConnection(*observer.LISTENER,timeout=5)
                connection.request('GET','/sync/meta?password=synthetic-query-secret')
                response=connection.getresponse();self.assertEqual(response.status,400)
                self.assertEqual(response.read(),b'');connection.close()
        self.assertEqual(actual.requests,[]);self.assertTrue(wire.failed)
        self.assertEqual(wire.wire()['requests'],[])
        self.assertEqual(log.getvalue(),'')

    def test_incomplete_upstream_body_cannot_be_counted_as_completion(self):
        with upstream(truncated=True):
            with observer.observing('observer-unit','failure','client_a','a'*64) as wire:
                connection=http.client.HTTPConnection(*observer.LISTENER,timeout=5)
                connection.request('POST','/sync/meta',body=b'{}')
                with self.assertRaises(http.client.RemoteDisconnected):connection.getresponse()
                connection.close()
            value=wire.wire()
        self.assertTrue(wire.failed)
        self.assertEqual(value['requests'][0]['disposition'],'incomplete')
        self.assertEqual(value['totals']['upstream_completed_count'],0)
        self.assertEqual(value['totals']['delivered_completed_count'],0)
        self.assertGreater(value['totals']['upstream_body_bytes'],0)

    def test_chunk_framing_failure_retains_consumed_payload_without_completion(self):
        prefix=observer.zstandard.ZstdCompressor().compress(b'OK')
        for failure,partial in [('truncated',b'ab'),('malformed',b'')]:
            with self.subTest(failure=failure),upstream(chunk_failure=failure):
                with observer.observing('observer-unit','chunk-failure','client_a','a'*64,armed=True) as wire:
                    connection=http.client.HTTPConnection(*observer.LISTENER,timeout=5)
                    connection.request('POST','/sync/upload',body=b'synthetic-collection')
                    with self.assertRaises(http.client.RemoteDisconnected):connection.getresponse()
                    connection.close()
                value=wire.wire()
            self.assertTrue(wire.failed)
            self.assertEqual(value['requests'][0]['upstream_status'],200)
            self.assertEqual(value['requests'][0]['disposition'],'incomplete')
            self.assertEqual(value['totals']['upstream_body_bytes'],len(prefix)+len(partial))
            self.assertEqual(value['totals']['upstream_completed_count'],0)
            self.assertEqual(value['totals']['delivered_completed_count'],0)
            self.assertEqual(value['totals']['delivered_body_bytes'],0)
            self.assertEqual(value['totals']['dropped_body_bytes'],len(prefix)+len(partial))
            self.assertFalse(value['fault']['accepted_upload_ack'])
            self.assertFalse(value['fault']['reply_dropped'])
            self.assertFalse(value['case_credit'])
            self.assertNotIn(prefix,encoded(value))
            self.assertNotIn(sha(prefix).encode(),encoded(value))

    def test_real_sdk_offline_preparation_retains_raw_cells_and_never_reads_credentials(self):
        # Only trusted input admission is stubbed. Authoring, SDK, projection and
        # finite offline dispatch are actual; there is no candidate service.
        with tempfile.TemporaryDirectory() as directory:
            root=pathlib.Path(directory);out=root/'out';out.mkdir();state=root/'state'
            store=Artifacts(out/'artifacts');identity=store.put(b'synthetic-public-input')
            action={'case':'normal-bidirectional','action':'prepare_rich'};invocation={'run_id':'offline-unit','source_commit':'0'*40}
            expect={'recipe_sha256':companion.RECIPE_SHA256,'selection_sha256':companion.SELECTION_SHA256,
                    'review_sha256':'a'*64,'baseline_sha256':None,'expected_sha256':None}
            hashes={name:identity for name in ('invocation.json','preflight.json','expectations.json')}
            with patch.object(companion,'inputs',return_value=(action,'client_b',invocation,expect,store,hashes)),\
                 patch.object(companion,'read_credentials',side_effect=AssertionError('offline secret read')),\
                 patch.object(companion,'observing',side_effect=AssertionError('offline HTTP')):
                self.assertEqual(companion.execute(root,out,state,'never-read'),0)
            observation=decode((out/'observations.json').read_bytes())
            self.assertFalse(observation['case_credit']);self.assertIsNone(observation['wire_sha256'])
            self.assertTrue(observation['observed']['operation_complete'])
            snapshot=store.json(observation['authority_sha256'])
            self.assertEqual(snapshot['format'],'meguri-retained-snapshot-v1')
            self.assertEqual(set(snapshot['tables']),{row[0] for row in companion.TABLES})
            columns=snapshot['tables']['config']['columns'];position=columns.index('val')
            self.assertTrue(all(set(row[position]) in ({'blob_hex'},{'text_hex'}) for row in snapshot['tables']['config']['rows']))
            stamps=copy.deepcopy(snapshot);stamps['tables']['cards']['rows'][0][4]={'integer':'999999999'}
            self.assertTrue(companion.collection_equal(snapshot,stamps))
            corrupted=copy.deepcopy(snapshot);corrupted['tables']['cards']['rows'][0][8]={'integer':'999999999'}
            self.assertFalse(companion.collection_equal(snapshot,corrupted))
            changed_media=copy.deepcopy(snapshot);changed_media['media_sha256']={}
            self.assertTrue(companion.compare(snapshot,changed_media))
            self.assertFalse(companion.compare(snapshot,changed_media,witness=True))

    def test_real_sdk_reverse_preparation_preserves_media_and_raw_wide_config_bytes(self):
        from final_desktop import author, change, media_map
        from final_contract import recipes
        with tempfile.TemporaryDirectory() as directory:
            root=pathlib.Path(directory);col,ids=author(api,root,'client_b',recipes())
            try:
                col.media.write_data('unchanged.txt',b'original media');before=media_map(col)
                col.set_config('Z_retained_cells',{'wide':9007199254740993,'unicode':'Straße'})
                raw=col.db.scalar("select val from config where key='Z_retained_cells'")
                kind=col.db.scalar("select typeof(val) from config where key='Z_retained_cells'")
                raw=raw.encode() if isinstance(raw,str) else bytes(raw)
                note=col.get_note(ids['notes']['keep']);note.tags=['Zulu','a','ß'];col.update_note(note)
                change(col,ids,recipes(),'reverse',with_media=False)
                self.assertEqual(media_map(col),before)
            finally:col.close(downgrade=False)
            snapshot=companion.snapshot(root/'client_b/collection.anki2',root/'client_b/collection.media')
            rows=snapshot['tables']['config']['rows']
            original=next(row[3] for row in rows if row[0] == {'text_hex':b'Z_retained_cells'.hex()})
            self.assertEqual(original,{'text_hex' if kind == 'text' else 'blob_hex':raw.hex()})
            keys=[bytes.fromhex(row[0]['text_hex']) for row in rows];self.assertEqual(keys,sorted(keys))
            tags=[bytes.fromhex(row[0]['text_hex']).decode() for row in snapshot['tables']['tags']['rows']]
            self.assertEqual(tags,sorted(tags,key=str.casefold))
            self.assertEqual(len(snapshot['tables']['revlog']['rows']),1)
            self.assertEqual(len(snapshot['tables']['notes']['rows']),2)
