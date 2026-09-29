import asyncio, copy, json, sys, tempfile, io
from PIL import Image
from pathlib import Path
sys.path.insert(0, str(Path.cwd() / 'apps/labello-wasm/tests'))
from playwright.async_api import async_playwright
from stylus_input import Scenario, application, require, until

OUT=Path('/tmp/labello-220-evidence')
async def run(mode):
    with application() as (origin, api, server):
        async with async_playwright() as p:
            browser=await p.chromium.launch(args=['--enable-unsafe-swiftshader','--use-gl=angle','--use-angle=swiftshader'])
            seed_context=await browser.new_context()
            s=Scenario(seed_context,origin,api)
            async def ready():
                try: return (await seed_context.request.get(api+'/health',timeout=1000)).ok
                except Exception: return False
            await until(ready,'server-ready')
            await s.seed('bounding_box')
            metadata=await s.request('GET','/datasets/stylus/admin')
            metadata['tasks'][0]['name']='Person'
            if mode == 'review':
                metadata['tasks'][0]['review']={'workflow':'approval','allowReviewerCorrections':True}
            task=copy.deepcopy(metadata['tasks'][0])
            task.update(taskId='skeleton:fixture',annotationType='skeleton',skeleton={'keypoints':[{'name':'center','required':True}],'edges':[],'allowHidden':True,'allowAbsent':False})
            metadata['tasks'].append(task)
            await s.request('PUT','/datasets/stylus/admin',data={k:metadata[k] for k in ['name','imageRoots','labelClasses','tasks','roleAssignments','imbalance','prelabelConfigs']})
            assignment=await s.request('POST','/datasets/stylus/images/next',data={'taskId':'skeleton:fixture' if mode == 'review' else 'bounding_box:fixture','kind':'annotation'})
            await s.request('POST','/datasets/stylus/assignments/complete',data={k:assignment[k] for k in ['assignmentId','imageId','taskId','kind']})
            availability=await s.request('GET','/datasets/stylus/assignments/availability?kind='+mode)
            require(availability['reasons']['bounding_box:fixture']==('nothing_awaiting_review' if mode=='review' else 'annotation_finished'),'fixture-availability-reason')
            storage=await seed_context.storage_state()
            matrix=[(w,h,d,False) for w,h in [(320,568),(390,844),(600,800),(1288,820),(1440,1000),(320,320)] for d in [1,2]]+[(390,844,3,False),(1288,820,1,True)]
            if len(sys.argv)>2: matrix=matrix[:1]
            for width,height,dpr,zoom in matrix:
                profile=None
                case_browser=None
                if zoom:
                    profile=tempfile.TemporaryDirectory(prefix='labello-220-zoom-')
                    extension=Path(profile.name)/'extension'; extension.mkdir()
                    (extension/'manifest.json').write_text(json.dumps({'manifest_version':3,'name':'Local verification zoom','version':'1.0','permissions':['tabs'],'background':{'service_worker':'background.js'}}))
                    (extension/'background.js').write_text('chrome.runtime.onInstalled.addListener(() => {});')
                    context=await p.chromium.launch_persistent_context(str(Path(profile.name)/'profile'),executable_path=p.chromium.executable_path,headless=True,ignore_default_args=['--disable-extensions'],args=['--enable-unsafe-swiftshader','--use-gl=angle','--use-angle=swiftshader',f'--force-device-scale-factor={dpr}',f'--disable-extensions-except={extension}',f'--load-extension={extension}'],viewport={'width':1440,'height':1000},device_scale_factor=dpr)
                    await context.add_cookies(storage['cookies'])
                else:
                    case_browser=await p.chromium.launch(args=['--enable-unsafe-swiftshader','--use-gl=angle','--use-angle=swiftshader',f'--force-device-scale-factor={dpr}'])
                    context=await case_browser.new_context(storage_state=storage,viewport={'width':1440,'height':1000},device_scale_factor=dpr)
                page=await context.new_page()
                errors=[]; claims=[]
                page.on('pageerror',lambda _:errors.append('pageerror'))
                page.on('request',lambda r: claims.append(1) if r.method=='POST' and r.url.endswith('/images/next') else None)
                await page.goto(origin+'/?api='+api+'&dataset=stylus')
                await page.locator('#startup-status').wait_for(state='detached',timeout=60000)
                async def canvas_ready():
                    return await page.evaluate("""() => { const c=document.getElementById('labello-canvas'); return Math.abs(c.width-c.clientWidth*devicePixelRatio)<=1 && Math.abs(c.height-c.clientHeight*devicePixelRatio)<=1; }""")
                await until(canvas_ready,'canvas-backing-dpr')
                await page.wait_for_timeout(1500)
                await page.mouse.click(150 if mode == 'review' else 64,28)
                await page.wait_for_timeout(1500)
                require(not claims,'claimed-before-acknowledgment')
                await page.set_viewport_size({'width':width,'height':height})
                if zoom:
                    worker=context.service_workers[0] if context.service_workers else await context.wait_for_event('serviceworker')
                    factor=await worker.evaluate("""async () => {
                        const tab=(await chrome.tabs.query({})).find(t => t.url && t.url.startsWith('http://127.0.0.1:'));
                        await chrome.tabs.setZoom(tab.id,2);
                        return await chrome.tabs.getZoom(tab.id);
                    }""")
                    require(factor==2,'actual-browser-zoom')
                await page.wait_for_timeout(600)
                await page.keyboard.press('Escape')
                await page.wait_for_timeout(150)
                require(not claims,'escape-acknowledged')
                require(abs(await page.evaluate('devicePixelRatio')-dpr*(2 if zoom else 1))<0.01,'observed-dpr')
                capture_attempts=0
                async def capture_rendered():
                    nonlocal capture_attempts
                    capture_attempts+=1
                    await page.mouse.move(width/2, height/2)
                    await page.wait_for_timeout(300)
                    data=await page.screenshot()
                    with Image.open(io.BytesIO(data)) as capture:
                        colors=capture.convert('RGB').getcolors(1000000)
                        if colors is not None and len(colors)<100: return False
                    (OUT/f'browser-{mode}-{width}x{height}-dpr{dpr}-zoom{2 if zoom else 1}.png').write_bytes(data)
                    return True
                try:
                    await until(capture_rendered,'rendered-screenshot',timeout=10)
                except AssertionError:
                    print(json.dumps(await page.evaluate('''() => { const c=document.getElementById('labello-canvas'); const gl=c?.getContext('webgl2'); return {tags:Array.from(document.body.children).map(e=>e.tagName),canvas:c ? {width:c.width,height:c.height,clientWidth:c.clientWidth,clientHeight:c.clientHeight,rect:c.getBoundingClientRect().toJSON(),lost:gl?.isContextLost()} : null,visibility:document.visibilityState,active:document.activeElement.tagName}; }''')),flush=True)
                    raise
                cdp=await context.new_cdp_session(page)
                tree=await cdp.send('Accessibility.getFullAXTree')
                require(any(n.get('role',{}).get('value')=='Canvas' for n in tree['nodes']),'canvas-accessibility')
                await page.keyboard.press('Tab')
                await page.wait_for_timeout(200)
                await page.keyboard.press('Tab')
                await page.wait_for_timeout(200)
                if len(sys.argv)>2: await page.screenshot(path=str(OUT/'browser-debug-focused.png'))
                await page.keyboard.press('Enter')
                async def claimed(): return len(claims)>0
                await until(claimed,'keyboard-acknowledgment',timeout=10)
                require(not errors,'browser-errors')
                print(json.dumps({'mode':mode,'browser':browser.version,'width':width,'height':height,'dpr':dpr,'zoom':2 if zoom else 1,'capture_attempts':capture_attempts,'claims_after_ack':len(claims),'passed':True}),flush=True)
                await context.close()
                if case_browser: await case_browser.close()
                if profile: profile.cleanup()
            await browser.close()
asyncio.run(run(sys.argv[1] if len(sys.argv)>1 else 'annotation'))
