#!/usr/bin/env python3
"""Opt-in real native kernel + real TUI, temporary HOME and loopback model only.
ZCODE_TEST_CJS=/absolute/zcode.cjs ZCODE_TEST_BIN=/absolute/zcode-tui python3 tests/native_model_pty.py
"""
import re, fcntl, http.server, json, os, pathlib, pty, select, shutil, struct, subprocess, tempfile, termios, threading, time

kernel = pathlib.Path(os.environ['ZCODE_TEST_CJS']).resolve()
binary = pathlib.Path(os.environ['ZCODE_TEST_BIN']).resolve()
requests = []
class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_): pass
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        requests.append(body['model'])
        assert body['model'] in ('model-one', 'model-two')
        text = 'reply-' + body['model']
        message = {'id':'msg_test','type':'message','role':'assistant','model':body['model'],
                   'content':[],'stop_reason':None,'stop_sequence':None,
                   'usage':{'input_tokens':10,'output_tokens':0}}
        events = [
            {'type':'message_start','message':message},
            {'type':'content_block_start','index':0,'content_block':{'type':'text','text':''}},
            {'type':'content_block_delta','index':0,'delta':{'type':'text_delta','text':text}},
            {'type':'content_block_stop','index':0},
            {'type':'message_delta','delta':{'stop_reason':'end_turn','stop_sequence':None},'usage':{'output_tokens':3}},
            {'type':'message_stop'}]
        if body.get('stream'):
            content = ''.join('event: '+e['type']+'\ndata: '+json.dumps(e)+'\n\n' for e in events).encode()
            kind='text/event-stream'
        else:
            content=json.dumps({**message,'content':[{'type':'text','text':text}], 'stop_reason':'end_turn'}).encode(); kind='application/json'
        self.send_response(200); self.send_header('Content-Type', kind); self.send_header('Content-Length', str(len(content))); self.end_headers()
        self.wfile.write(content)

with tempfile.TemporaryDirectory(prefix='zcode-model-pty-') as temp:
    home=pathlib.Path(temp)
    server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler)
    threading.Thread(target=server.serve_forever,daemon=True).start()
    config={'provider':{'test':{'kind':'anthropic','name':'Loopback test',
            'options':{'baseURL':f'http://127.0.0.1:{server.server_port}','apiKey':'synthetic'},
            'models':{'model-one':{'name':'AAA first'},'model-two':{'name':'ZZZ second'}}}},
            'model':{'main':'test/model-one','lite':'test/model-one'}}
    (home/'.zcode/cli').mkdir(parents=True)
    (home/'.zcode/cli/config.json').write_text(json.dumps(config))
    app=home/'app'; app.mkdir()
    (app/'resources').symlink_to(kernel.parent.parent)
    env={'HOME':temp,'PATH':os.environ['PATH'],'TERM':'xterm-256color',
         'XDG_CONFIG_HOME':str(home/'.config'),'XDG_CACHE_HOME':str(home/'.cache'),
         'XDG_DATA_HOME':str(home/'.local/share'),'ZCODE_APP':str(app),
         'ZCODE_BASE_URL':'http://127.0.0.1:1','ZCODE_TUI_NO_UPDATE_CHECK':'1'}
    prefix=home/'installed'; (prefix/'bin').mkdir(parents=True)
    shutil.copy2(binary,prefix/'bin/zcode-tui')
    subprocess.run(['bash','install.sh','--no-build','--prefix',str(prefix)],env=env,check=True,capture_output=True)
    # The native registry uses the official migrated provider configuration.
    # Seed it through the official CLI against our loopback provider only.
    subprocess.run([str(prefix/'bin/zcode'), '--mode', 'build', '--prompt', 'Reply briefly; no tools.'],
                   env=env, cwd=temp, capture_output=True, check=True, timeout=40)
    requests.clear()
    env['ZCODE_TUI_ZCODE_BIN']=str(prefix/'bin/zcode')
    env['ZCODE_TUI_LOG']=str(home/'protocol.log')
    master,slave=pty.openpty(); fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',38,130,0,0))
    process=subprocess.Popen([str(prefix/'bin/zcode-tui')],stdin=slave,stdout=slave,stderr=slave,env=env,cwd=temp,start_new_session=True)
    os.close(slave); transcript=bytearray()
    def pump(seconds):
        deadline=time.monotonic()+seconds
        while time.monotonic()<deadline:
            if select.select([master],[],[],min(.1,max(0,deadline-time.monotonic())))[0]:
                try:
                    chunk=os.read(master,65536);transcript.extend(chunk)
                    if b'\x1b[6n' in chunk: os.write(master,b'\x1b[1;1R')
                except OSError:return
    def send(text):os.write(master,text.encode());pump(.3)
    def until(predicate,timeout=25):
        deadline=time.monotonic()+timeout
        while not predicate():
            assert time.monotonic()<deadline, 'timeout; '+re.sub(r'\x1b\[[0-?]*[ -/]*[@-~]', '', transcript.decode(errors='replace'))[-6000:]+'\nLOG\n'+((home/'protocol.log').read_text() if (home/'protocol.log').exists() else '')
            pump(.1)
    try:
        until(lambda:b'Welcome' in transcript)
        # Catalog must arrive before any prompt (the regression being fixed).
        until(lambda:(home/'protocol.log').exists() and 'session/close' in (home/'protocol.log').read_text())
        pump(1);send('/model\r');until(lambda:b'model-two' in transcript or b'ZZZ' in transcript)
        assert requests==[], 'opening model picker sent a model request'
        send('\x1b[B');send('\r');send('Reply briefly; do not use tools.\r')
        until(lambda:requests and requests[-1]=='model-two')
        until(lambda:b'turn.completed' in (home/'protocol.log').read_bytes())
        pump(.5)
        first_turn_requests = len(requests)
        assert first_turn_requests >= 1 and all(m == 'model-two' for m in requests), requests
        send('/model\r');send('\x1b[A');send('\r')
        until(lambda:(home/'protocol.log').read_text().count('session/setModel')>=2)
        pump(.5);send('Reply briefly again; do not use tools.\r')
        until(lambda:requests and requests[-1]=='model-one')
        until(lambda:(home/'protocol.log').read_text().count('turn.completed')>=2)
        assert requests[first_turn_requests:] and all(m == 'model-one' for m in requests[first_turn_requests:]), requests
        send('/exit\r');process.wait(timeout=5)
        assert process.returncode==0
        print('PASS: catalog before first prompt; selected model used on first turn; live switch used on second turn; clean exit')
        print('Observed model requests:', requests)
    finally:
        if process.poll() is None:
            send('\x1b');send('/exit\r')
            try:process.wait(timeout=5)
            except subprocess.TimeoutExpired:process.terminate();process.wait(timeout=5)
        os.close(master);server.shutdown()
        time.sleep(.5)
