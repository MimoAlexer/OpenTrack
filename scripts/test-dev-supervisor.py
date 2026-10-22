#!/usr/bin/env python3
"""Exercise launcher child ownership and credential separation using harmless stubs."""
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import stat
import subprocess
import tempfile
import time
import unittest

SOURCE = Path(__file__).with_name('dev.sh')

class Launcher(unittest.TestCase):
    def test_separate_child_credentials_overrides_and_owned_shutdown(self):
        self.exercise_launcher(default_storage=False)

    def test_default_storage_is_private_with_permissive_parent_umask(self):
        self.exercise_launcher(default_storage=True)

    def exercise_launcher(self, *, default_storage):
        parent_environment = dict(os.environ)
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            for path in ['scripts','bin','target/debug','web']:
                (root/path).mkdir(parents=True)
            shutil.copy(SOURCE,root/'scripts/dev.sh')
            def script(path,text):
                (root/path).write_text(text); (root/path).chmod(0o700)
            script('scripts/dev-db.sh','#!/bin/bash\nset -eu\nif [[ "$1" = env-path ]]; then printf "%s/env\\n" "$(dirname "$(dirname "$0")")"; fi\n')
            script('bin/cargo','#!/bin/sh\nexit 0\n')
            script('bin/npm','#!/bin/sh\nexit 0\n')
            script('target/debug/opentrack-server','''#!/usr/bin/env python3
import json,os,signal,sys,time
mode=sys.argv[1]
mask=os.umask(0);os.umask(mask)
secret_names=[k for k in os.environ if k.startswith('OPENTRACK_') and ('DATABASE_URL' in k or 'SETUP_TOKEN' in k)]
with open('child-'+mode+'.json.tmp','w') as output:
 json.dump({'pid':os.getpid(),'mode':mode,'keys':sorted(secret_names),'runtime_override':os.environ.get('OPENTRACK_DATABASE_URL','').startswith('postgres://custom_api:'),'worker_override':os.environ.get('OPENTRACK_WORKER_DATABASE_URL','').startswith('postgres://custom_worker:'),'storage_root':os.environ.get('OPENTRACK__STORAGE__LOCAL_ROOT'),'umask':mask},output)
os.replace('child-'+mode+'.json.tmp','child-'+mode+'.json')
signal.signal(signal.SIGTERM,lambda *_:sys.exit(0))
while True: time.sleep(.02)
''')
            (root/'env').write_text("export OPENTRACK_DATABASE_URL='postgres://generated_api:private@127.0.0.1:55432/example'\nexport OPENTRACK_MIGRATION_DATABASE_URL='postgres://owner:private@127.0.0.1:55432/example'\nexport OPENTRACK_WORKER_DATABASE_URL='postgres://generated_worker:private@127.0.0.1:55432/example'\nexport OPENTRACK_OPERATOR_DATABASE_URL='postgres://operator:private@127.0.0.1:55432/example'\nexport OPENTRACK_SETUP_TOKEN='private-setup-token'\nexport OPENTRACK_ENVIRONMENT=development\n")
            with socket.socket() as probe:
                probe.bind(('127.0.0.1',0)); port=probe.getsockname()[1]
            environment={k:v for k,v in os.environ.items() if not k.startswith('OPENTRACK')}
            data_home=root/'private-user-data'
            data_home.mkdir(mode=0o700)
            environment['XDG_DATA_HOME']=str(data_home)
            expected_storage=data_home/'opentrack-server'/'blobs' if default_storage else root/'blobs'
            if not default_storage:
                environment['OPENTRACK__STORAGE__LOCAL_ROOT']=str(expected_storage)
            environment.update(PATH=str(root/'bin')+':'+environment['PATH'],OPENTRACK_LISTEN=f'127.0.0.1:{port}',OPENTRACK_DATABASE_URL='postgres://custom_api:private@127.0.0.1:55432/example',OPENTRACK_WORKER_DATABASE_URL='postgres://custom_worker:private@127.0.0.1:55432/example')
            unrelated=subprocess.Popen(['sleep','30'])
            launcher=subprocess.Popen(['bash','scripts/dev.sh'],cwd=root,env=environment,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,umask=0o002)
            try:
                deadline=time.monotonic()+5
                while not ((root/'child-serve.json').exists() and (root/'child-worker.json').exists()):
                    if launcher.poll() is not None:
                        stdout,stderr=launcher.communicate();self.fail(f'Launcher exited before children: {stdout} {stderr}')
                    self.assertLess(time.monotonic(),deadline,'Launcher children did not start')
                    time.sleep(.01)
                api=json.loads((root/'child-serve.json').read_text());worker=json.loads((root/'child-worker.json').read_text())
                self.assertEqual(api['keys'],['OPENTRACK_DATABASE_URL','OPENTRACK_SETUP_TOKEN'])
                self.assertEqual(worker['keys'],['OPENTRACK_WORKER_DATABASE_URL'])
                self.assertTrue(api['runtime_override']);self.assertTrue(worker['worker_override'])
                for child in [api,worker]:
                    self.assertEqual(child['storage_root'],str(expected_storage))
                    self.assertEqual(child['umask'],0o002,'Private provisioning changed the inherited child umask')
                if default_storage:
                    for path in [expected_storage.parent,expected_storage]:
                        self.assertTrue(path.is_dir())
                        self.assertEqual(stat.S_IMODE(path.stat().st_mode),0o700,f'{path.name} was not private')
                        self.assertEqual(path.stat().st_uid,os.getuid())
                launcher.send_signal(signal.SIGTERM);stdout,stderr=launcher.communicate(timeout=5)
                for secret in ['private-setup-token',':private@']:
                    self.assertNotIn(secret,stdout+stderr)
                self.assertEqual(launcher.returncode,143)
                for child in [api,worker]:
                    with self.assertRaises(ProcessLookupError):os.kill(child['pid'],0)
                self.assertIsNone(unrelated.poll(),'Unrelated process was stopped')
            finally:
                if launcher.poll() is None:
                    launcher.terminate()
                    try:launcher.communicate(timeout=5)
                    except subprocess.TimeoutExpired:launcher.kill();launcher.communicate()
                unrelated.terminate();unrelated.wait(timeout=5)
        self.assertEqual(dict(os.environ),parent_environment,'Launcher test mutated the parent environment')

if __name__=='__main__':unittest.main()
