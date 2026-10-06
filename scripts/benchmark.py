"""Real Windows ConPTY benchmark; owns an isolated app session and its processes.

Requires psutil, pywinpty and pyte. Does not alter Spotify credentials, queues,
or the user's music. Refuses to use the real local app-data directory as a fixture.
The local app-data fixture uses the installed tools through a directory junction.
"""
import argparse
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import threading
import time
import psutil
import pyte

from winpty import PtyProcess, Backend

class Session:
    def __init__(self, exe, data_root, args=()):
        self.output = bytearray()
        self.samples = []
        self.phase = 'startup'
        self.screen = pyte.Screen(120,32)
        self.decoder = pyte.Stream(self.screen)
        self.stop_sampling = threading.Event()
        env = dict(os.environ, LOCALAPPDATA=str(data_root))
        self.started = time.perf_counter()
        self.pty = PtyProcess.spawn([str(exe), *args], cwd=str(exe.parent), env=env,
            dimensions=(32, 120), backend=Backend.ConPTY)
        self.pid = self.pty.pid
        self.reader = threading.Thread(target=self.read_output, daemon=True)
        self.reader.start()
        self.sampler = threading.Thread(target=self.sample, daemon=True)
        self.sampler.start()

    def read_output(self):
        try:
            while self.pty.isalive():
                text = self.pty.read(16384)
                self.decoder.feed(text)
                self.output.extend(text.encode('utf-8'))
        except (EOFError, OSError):
            pass

    def sample(self):
        try:
            main = psutil.Process(self.pid)
        except psutil.NoSuchProcess:
            return
        owned = {main.pid: main}
        while not self.stop_sampling.is_set():
            processes = []
            try:
                tree = [main, *main.children(recursive=True)]
            except psutil.Error:
                break
            for process in tree: owned[process.pid] = process
            for process in list(owned.values()):
                try:
                    if not process.is_running(): continue
                    memory = process.memory_info()
                    cpu = process.cpu_times()
                    processes.append({'pid':process.pid, 'name':process.name(), 'rss':memory.rss,
                        'private':getattr(memory, 'private', memory.vms), 'cpu_seconds':cpu.user+cpu.system, 'threads':process.num_threads()})
                except psutil.Error:
                    pass
            self.samples.append({'seconds':time.perf_counter()-self.started, 'phase':self.phase, 'processes':processes})
            self.stop_sampling.wait(.1)

    def send(self, keys):
        for character in keys:
            self.pty.write(character)
            time.sleep(.002)

    def until(self, pattern, start=0, timeout=15):
        deadline = time.perf_counter()+timeout
        while time.perf_counter() < deadline:
            text = bytes(self.output[start:]).decode('utf-8', errors='replace')
            text = re.sub(r'\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][^\x07]*(?:\x07|\x1b\\)', '', text)
            if re.search(pattern, text, re.I):
                return time.perf_counter()
            if not self.pty.isalive():
                raise RuntimeError('App exited before the expected frame: '+re.sub(r'https?://\S+', '[url]', text)[-1200:])
            time.sleep(.01)
        raise TimeoutError('Expected terminal output: '+pattern+'; last output: '+re.sub(r'https?://\S+', '[url]', text)[-700:])

    def search(self, query, previous=''):
        self.send('1/'+'\x08'*len(previous)+query)
        deadline=time.perf_counter()+5
        def visible_rows():
            return [re.sub('[\u2580-\u259f]', ' ', line) for line in self.screen.display]
        while not ('SEARCH' in visible_rows()[3] and query in visible_rows()[4]):
            if time.perf_counter()>deadline:
                raise AssertionError('Search editor did not contain the exact benchmark query: '+repr(visible_rows()[4]))
            time.sleep(.01)
        start=len(self.output); begin=time.perf_counter()
        self.send('\r')
        wanted = ('Get Lucky','Daft Punk') if query == 'Daft Punk Get Lucky' else ('Creep','Radiohead')
        deadline=time.perf_counter()+20
        while not any(all(word in line for word in wanted) for line in visible_rows()[8:24]):
            if time.perf_counter()>deadline:
                raise TimeoutError('Music result row did not become visible for '+query)
            time.sleep(.005)
        return time.perf_counter()-begin

    def shutdown(self):
        try:
            self.send('q')
            deadline = time.perf_counter() + 10
            while self.pty.isalive() and time.perf_counter() < deadline:
                time.sleep(.05)
        finally:
            self.stop_sampling.set(); self.sampler.join(2)
            self.pty.close(force=True)
            self.reader.join(2)

def phase_stats(samples):
    report = {}
    for phase in sorted({s['phase'] for s in samples}):
        rows = [s for s in samples if s['phase']==phase and s['processes']]
        if not rows:
            continue
        totals = [sum(p['rss'] for p in s['processes'])/1048576 for s in rows]
        privates = [sum(p['private'] for p in s['processes'])/1048576 for s in rows]
        names = sorted({p['name'] for s in rows for p in s['processes']})
        cpu_delta = 0
        for a,b in zip(rows, rows[1:]):
            previous = {p['pid']:p['cpu_seconds'] for p in a['processes']}
            cpu_delta += sum(max(0,p['cpu_seconds']-previous.get(p['pid'],p['cpu_seconds'])) for p in b['processes'])
        duration = rows[-1]['seconds']-rows[0]['seconds']
        report[phase] = {'rss_mib_median':statistics.median(totals), 'rss_mib_peak':max(totals),
            'private_mib_peak':max(privates), 'cpu_one_core_percent':100*cpu_delta/max(.001,duration),
            'processes':names, 'duration_seconds':duration}
    return report

def run(exe, root, trial, audio, idle_seconds, appearance=None):
    state = root/'Tuitify'
    state.mkdir(parents=True, exist_ok=True)
    for folder in [state,state/'youtube']:
        for name in ['queue.json','cache.json','stats.json']:
            file=folder/name
            if file.exists(): file.unlink()
    settings={'discord_rpc':False,'volume':0}
    if appearance:
        original=json.loads(appearance.read_text(encoding='utf-8-sig'))
        settings.update({key:original[key] for key in ['theme','background_image','background_image_vertical','background_dim'] if key in original})
    (state/'config.json').write_text(json.dumps(settings),encoding='utf-8')
    (state/'youtube').mkdir(exist_ok=True)
    (state/'youtube'/'config.json').write_text(json.dumps(settings),encoding='utf-8')
    player = Session(exe, root)
    result = {'trial':trial}
    try:
        deadline=time.perf_counter()+15
        while not ('TUITIFY' in player.screen.display[0] and 'v0.' in player.screen.display[0]):
            if time.perf_counter()>deadline:
                raise TimeoutError('The application header did not become visible')
            time.sleep(.005)
        result['startup_to_first_frame_seconds'] = time.perf_counter()-player.started
        player.phase='idle'; time.sleep(5)
        player.phase='search_cold'
        result['search_cold_seconds']=player.search('Daft Punk Get Lucky')
        player.phase='search_warm'; time.sleep(2)
        # Existing text is intentionally replaced using the real editor's Backspace.
        result['search_warm_seconds']=player.search('Radiohead Creep','Daft Punk Get Lucky')
        player.phase='search_cached'
        result['search_cached_seconds']=player.search('Daft Punk Get Lucky','Radiohead Creep')
        player.phase='idle_after_search'; time.sleep(idle_seconds)
        if audio:
            player.phase='playback'; start=len(player.output); begin=time.perf_counter()
            player.send('\r')
            player.until('▶ PLAYING|requires sign-in|rate limit|blocked this connection|playback.*failed',start,20)
            response=bytes(player.output[start:]).decode('utf-8',errors='replace')
            result['playback']='blocked_by_upstream' if re.search('requires sign-in|rate limit|blocked this connection|playback.*failed',response,re.I) else 'playing'
            result['playback_response_seconds']=time.perf_counter()-begin
            if result['playback']=='playing':
                time.sleep(5)
                player.send(' '); time.sleep(1); player.send(' '); time.sleep(2)
                start=len(player.output); begin=time.perf_counter(); player.send('n')
                player.until('PLAYING|requires sign-in|rate limit|blocked this connection',start,20)
                result['next_track_response_seconds']=time.perf_counter()-begin
        settled=[row for row in player.samples if row['phase']=='idle_after_search'][-10:]
        result['settled_idle_rss_mib']=statistics.median(sum(p['rss'] for p in row['processes'])/1048576 for row in settled)
        if idle_seconds >= 30:
            player.phase='cached_after_idle'
            result['cached_after_idle_seconds']=player.search('Radiohead Creep','Daft Punk Get Lucky')
        result['resources']=phase_stats(player.samples)
    finally:
        player.shutdown()
    return result

if __name__=='__main__':
    parser=argparse.ArgumentParser()
    parser.add_argument('--exe',type=Path,required=True)
    parser.add_argument('--root',type=Path,required=True)
    parser.add_argument('--out',type=Path,required=True)
    parser.add_argument('--trials',type=int,default=3)
    parser.add_argument('--audio',action='store_true')
    parser.add_argument('--idle-seconds',type=float,default=5)
    parser.add_argument('--appearance',type=Path)
    options=parser.parse_args()
    if options.root.resolve() == Path(os.environ['LOCALAPPDATA']).resolve():
        parser.error('Use an isolated benchmark fixture, never the real local app-data directory.')
    options.root.mkdir(parents=True,exist_ok=True)
    report={'exe':str(options.exe.resolve()),'trials':[], 'memory_scope':'Tuitify and owned descendants (including helper consoles); excludes external ConPTY host and benchmark Python', 'cpu_scope':'percent of one CPU core', 'volume':0}
    try:
        for trial in range(options.trials):
            report['trials'].append(run(options.exe.resolve(),options.root.resolve(),trial,options.audio and trial==0,options.idle_seconds,options.appearance))
            options.out.write_text(json.dumps(report,indent=2),encoding='utf-8')
            print(json.dumps(report['trials'][-1]),flush=True)
    except Exception as error:
        report['error']=str(error)
        options.out.write_text(json.dumps(report,indent=2),encoding='utf-8')
        raise
