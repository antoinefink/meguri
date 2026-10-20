import CardRendering
import CoreGraphics
import Foundation
import Testing
import WebKit

#if os(macOS)
import AppKit
#endif

extension WebKitSuites.CardRuntimeSuites {
    /// Neutral M14 expectations. Pixel coordinates, conversion values and
    /// sizing arithmetic below never call production geometry helpers.
    @Suite struct ImageOcclusionReviewTests {
        static func fixture(_ name: String) throws -> String {
            let url = try #require(Bundle.module.url(forResource: name, withExtension: "html", subdirectory: "Fixtures/image-occlusion"))
            return try String(contentsOf: url, encoding: .utf8)
        }

        static func side(_ number: Int = 1, answer: Bool = false) throws -> String {
            try fixture("page")
                .replacingOccurrences(of: "{{shapes}}", with: fixture("IO\(number)-\(answer ? "answer" : "question")"))
                .replacingOccurrences(of: "{{button}}", with: answer ? "<button id=\"toggle\">Masks</button>" : "")
        }

        static func prepare(_ harness: WebTestHarness, _ page: TestPage) async throws {
            harness.media.add("/_io.png", TestMedia.png(width: 400, height: 300, color: CGColor(red: 128.0/255, green: 128.0/255, blue: 128.0/255, alpha: 1)), "image/png")
            try await page.run("""
                window.__ioPasses = 0; window.__ioResolved = []; window.__ioData = null; window.__ioDraws = 0; window.__ioDrawnSide = 0;
                window.__ioOptions = {onWillDrawShapes: function(d,c) { window.__ioData = d; window.__ioPasses++; }};
                """)
        }

        /// `shown` and setup fulfilment both precede IO's detached draw. This
        /// test-only adapter observes the end of a successful pass, including
        /// hooks that return replacement data without incrementing __ioPasses.
        /// Read the live did hook once, preserving its receiver and exceptions.
        static func showDrawn(_ page: TestPage, _ html: String, answer: Bool = false) async throws {
            let before = try #require(try await page.run("return __ioDraws;") as? Int)
            let observed = html.replacingOccurrences(of: "anki.imageOcclusion.setup(window.__ioOptions)", with: """
                anki.imageOcclusion.setup(new Proxy(window.__ioOptions, {get:function(options,key){
                  const value=Reflect.get(options,key);
                  if(key!=='onDidDrawShapes' || (value!=null && typeof value!=='function')) return value;
                  return function(data,context){
                    if(value!=null) Reflect.apply(value,undefined,[data,context]);
                    window.__ioDraws++;
                    window.__ioDrawnSide=\(before + 1);
                  };
                }}))
                """)
            if answer { try await page.showPresentedAnswer(observed) }
            else { try await page.showPresentedQuestion(observed) }
            try await page.harness.until("completed IO draw for this side") {
                try await page.bool("__ioDraws > \(before) && __ioDrawnSide === \(before + 1)")
            }
        }

        static func passes(_ page: TestPage, _ count: Int) async throws {
            try await page.harness.until("IO pass \(count)") { try await page.bool("window.__ioPasses >= \(count)") }
        }

        static func frames(_ page: TestPage) async throws {
            try await page.run("await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(() => requestAnimationFrame(r))));")
        }

        static func pixel(_ page: TestPage, _ x: String, _ y: String) async throws -> [Int] {
            try await page.json([Int].self, """
                const c = document.getElementById('image-occlusion-canvas'), W = c.width, H = c.height;
                return JSON.stringify(Array.from(c.getContext('2d').getImageData(Math.floor(\(x)), Math.floor(\(y)), 1, 1).data));
                """)
        }

        static let red = [255, 0, 0, 255]
        static let blue = [0, 0, 255, 255]
        static let white = [255, 255, 255, 255]
        static let clear = [0, 0, 0, 0]

        /// MC1–4, TG1–6, AP6/AP8, all platform documents and appearances.
        @Test(arguments: CardRuntime.Platform.allCases, CardRuntime.Appearance.allCases)
        func masksToggleRegistryAndVisuals(platform: CardRuntime.Platform, appearance: CardRuntime.Appearance) async throws {
            try await WebTestHarness.withPage(platform: platform, appearance: appearance, size: CGSize(width: 480, height: 650)) { harness, page async throws in
                try await Self.prepare(harness, page)
                // Define every test-owned global before taking the globals baseline.
                let names = try await page.json([String].self, GlobalsSurfaceTests.windowNames)
                for number in [1, 2] {
                    try await Self.showDrawn(page, Self.side(number))
                    try await Self.expectCurrentSize(page)
                    #expect(try await page.bool("document.getElementById('image-occlusion-container').clientHeight===300"))
                    #expect(try await Self.pixel(page, ".5*W", ".175*H") == Self.red)
                    #expect(try await Self.pixel(page, ".15*W", ".175*H") == (number == 1 ? Self.blue : Self.clear))
                    #expect(try await Self.pixel(page, ".8*W", ".175*H") == (number == 1 ? Self.blue : Self.clear))
                    #expect(try await Self.pixel(page, "Math.round(.05*W)+2", "Math.round(.6*H)+2") == Self.white)
                    #expect(try await Self.pixel(page, ".5*W", ".45*H") == Self.clear)
                    let beforeKey = try await page.string("__ioPasses")
                    try await page.run("window.dispatchEvent(new KeyboardEvent('keydown', {key:'M'}));")
                    #expect(try await page.string("__ioPasses") == beforeKey)
                    if number == 1 { try await Self.snapshot(page, "\(platform.rawValue)-\(appearance.rawValue)-question") }
                    try await Self.showDrawn(page, Self.side(number, answer: true), answer: true)
                    #expect(try await Self.pixel(page, ".5*W", ".175*H") == Self.clear)
                    let edge = try await Self.pixel(page, "Math.round(.4*W)", ".175*H")
                    #expect(edge[3] > 0 && edge[1] > edge[0] && edge[1] > edge[2])
                    #expect(try await page.string("document.getElementById('toggle').style.display") == (number == 2 ? "none" : ""))
                    for selector in ["#image-occlusion-canvas", "#image-occlusion-container", "#image-occlusion-container img"] {
                        #expect(try await !page.hasCardListener("tap", at: selector))
                    }
                    #expect(try await page.hasCardListener("keydown"))
                    #expect(try await page.hasCardListener("tap", at: "#toggle") == (number == 1))
                    if number == 1 {
                        try await Self.snapshot(page, "\(platform.rawValue)-\(appearance.rawValue)-answer")
                        try await page.run("document.getElementById('toggle').click();")
                        #expect(try await Self.pixel(page, ".15*W", ".175*H") == Self.clear)
                        #expect(try await Self.pixel(page, "Math.round(.4*W)", ".175*H") == Self.clear)
                        #expect(try await page.bool("__ioData.activeShapes.length === 1 && __ioData.inactiveShapes.length === 0 && __ioData.highlightShapes.length === 0"))
                        #expect(try await Self.pixel(page, "Math.round(.05*W)+2", "Math.round(.6*H)+2") == Self.white)
                        try await page.run("window.dispatchEvent(new KeyboardEvent('keydown', {key:'M'}));")
                        #expect(try await Self.pixel(page, ".15*W", ".175*H") == Self.blue)
                        // CSS hiding does not disable M; lowercase m never toggles.
                        try await page.run("document.getElementById('toggle').style.visibility='hidden'; document.head.insertAdjacentHTML('beforeend','<style>#toggle { display:none }</style>');")
                    }
                    let before = try await page.string("__ioPasses")
                    try await page.run("window.dispatchEvent(new KeyboardEvent('keydown', {key:'m'}));")
                    #expect(try await page.string("__ioPasses") == before)
                    try await page.run("window.dispatchEvent(new KeyboardEvent('keydown', {key:'M'}));")
                    if number == 2 {
                        try await page.run("document.getElementById('toggle').click();")
                        #expect(try await page.string("__ioPasses") == before)
                    } else {
                        #expect(try await Self.pixel(page, ".15*W", ".175*H") == Self.clear)
                    }
                    #expect(try await page.string("document.getElementById('err').textContent") == "")
                    #expect(try await page.bool("document.getElementById('image-occlusion-canvas').getAttribute('style')===null && document.querySelector('#image-occlusion-container img').getAttribute('style')===null && Array.from(document.getElementById('image-occlusion-container').style).join(',')==='aspect-ratio'"))
                }
                #expect(Set(try await page.json([String].self, GlobalsSurfaceTests.windowNames)) == Set(names))
                #expect(harness.diagnostics.isEmpty)
            }
        }

        /// MC5–9, GE1–4 and HK8, using every remaining valid golden.
        @Test func colorsGeometryAndShapeSelection() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await Self.prepare(harness, page)
                try await Self.showDrawn(page, Self.side(3))
                for (x, y, color) in [(".125*W", ".125*H", Self.red), (".375*W", ".125*H", [0,255,0,255]),
                                      (".625*W", ".125*H", Self.blue), (".875*W", ".125*H", [255,235,162,255]),
                                      (".15*W", ".6*H", Self.blue), (".5*W", ".55*H", [255,0,255,255])] {
                    #expect(try await Self.pixel(page, x, y) == color)
                }
                try await Self.showDrawn(page, Self.side(5))
                #expect(try await Self.pixel(page, "Math.round(.25*W)-Math.round(.125*H)/2", "Math.round(.25*H)+Math.round(.25*W)/2") == Self.red)
                #expect(try await Self.pixel(page, "Math.round(.25*W)+Math.round(.25*W)/2", "Math.round(.25*H)+Math.round(.125*H)/2") == Self.clear)
                #expect(try await Self.pixel(page, "Math.round(.6*W)-Math.round(.05*H)", "Math.round(.2*H)+Math.round(.1*W)") == Self.red)
                #expect(try await Self.pixel(page, "Math.round(.6*W)+Math.round(.1*W)", "Math.round(.2*H)+Math.round(.05*H)") == Self.clear)
                #expect(try await page.bool("__ioData.activeShapes[0].angle === 90 && __ioData.activeShapes[2].angle === 90"))
                #expect(try await Self.pixel(page, ".65*W", ".65*H") == Self.clear)
                try await Self.showDrawn(page, Self.side(6))
                #expect(try await Self.pixel(page, ".6*W", ".5667*H") == Self.red)
                #expect(try await Self.pixel(page, ".2*W", ".1667*H") == Self.clear)
                for (x,y) in [(".2*W", ".7*H"), ("Math.round(.1*W)+2", "Math.round(.6*H)+2"), ("Math.round(.3*W)-2", "Math.round(.8*H)-2")] {
                    #expect(try await Self.pixel(page, x, y) == Self.red)
                }
                #expect(try await page.bool("__ioData.activeShapes[0].points[0].x === '.1' && __ioData.activeShapes[0].points[0].y === '.1'"))
                try await Self.showDrawn(page, Self.side(7))
                #expect(try await Self.pixel(page, ".2*W", ".2*H") == Self.red)
                #expect(try await Self.pixel(page, ".6*W", ".6*H") == Self.blue)
                #expect(try await page.bool("__ioData.activeShapes[0].ordinal === 1"))
                try await Self.showDrawn(page, Self.side(9))
                #expect(try await page.bool("__ioData.activeShapes.length === 3"))
                for (x,y) in [(".2*W", ".2*H"), (".5*W", ".2*H"), (".2*W", ".6*H")] {
                    #expect(try await Self.pixel(page, x, y) == Self.clear)
                }
                #expect(try await Self.pixel(page, ".7*W", ".7*H") == Self.red)
                try await Self.showDrawn(page, Self.side(10))
                #expect(try await page.bool("__ioData.activeShapes.length === 1 && __ioData.inactiveShapes.length === 1"))
                #expect(try await Self.pixel(page, ".25*W", ".25*H") == Self.red)
                #expect(try await Self.pixel(page, ".65*W", ".65*H") == Self.blue)
                let noColors = try Self.side().replacingOccurrences(of: "--", with: "--unused-")
                try await Self.showDrawn(page, noColors)
                #expect(try await Self.pixel(page, ".5*W", ".175*H") == [255,142,142,255])
                #expect(try await Self.pixel(page, ".15*W", ".175*H") == [255,235,162,255])
                let edge = try await Self.pixel(page, "Math.round(.4*W)", ".175*H")
                #expect(edge[3] > 0 && edge[0] < 200 && edge[1] < 100 && edge[2] < 100)
                #expect(harness.diagnostics.isEmpty)
            }
        }

        /// AP1–5 and the full M4 own property/default/conversion contract.
        @Test(arguments: CardRuntime.Platform.allCases)
        func apiClassesAndConversions(platform: CardRuntime.Platform) async throws {
            try await WebTestHarness.withPage(platform: platform) { harness, page async throws in
                #expect(try await page.bool("anki.setupImageCloze === anki.imageOcclusion.setup"))
                #expect(try await page.string("Object.keys(anki.imageOcclusion).join(',')") == "setup,drawShape,Ellipse,Polygon,Rectangle,Shape,Text")
                #expect(try await page.bool("['imageOcclusion','setupImageCloze'].every(k => { const d=Object.getOwnPropertyDescriptor(anki,k); return d.writable && d.enumerable && d.configurable; })"))
                #expect(try await page.bool("""
                    (() => {
                      const a = anki.imageOcclusion;
                      const base = 'left,top,angle,fill,occludeInactive,ordinal,id';
                      return [[a.Shape,''],[a.Rectangle,',width,height'],[a.Ellipse,',rx,ry'],[a.Polygon,',points'],[a.Text,',text,scaleX,scaleY,fontSize']].every(([C,keys]) => {
                        const s = new C();
                        return Object.keys(s).join(',') === base+keys && s instanceof a.Shape && s.left===0 && s.top===0 && s.angle===0 &&
                          s.fill === (C===a.Text ? '#000000' : '#ffeba2') && s.occludeInactive===undefined && s.ordinal===undefined &&
                          (C===a.Shape ? s.id===undefined : /^(rect|ellipse|polygon|text)-[0-9]+$/.test(s.id)) &&
                          s.toDataForCloze===undefined && s.toFabric===undefined;
                      }) && new a.Rectangle().width===0 && new a.Rectangle().height===0 && new a.Ellipse().rx===0 && new a.Ellipse().ry===0 &&
                      new a.Polygon().points.length===0 && new a.Text().text==='' && new a.Text().scaleX===1 && new a.Text().scaleY===1 && new a.Text().fontSize===undefined;
                    })()
                    """))
                #expect(try await page.bool("""
                    (() => {
                      const a=anki.imageOcclusion, size={width:200,height:100};
                      const r=new a.Rectangle({left:.25,top:.5,width:.125,height:.25,angle:-90,fill:null,ordinal:7,occludeInactive:true,ignored:5});
                      const abs=r.toAbsolute(size), normal=abs.toNormal(size);
                      const p=new a.Polygon({points:[{x:'.25',y:'.5'},{x:'',y:'bad'}]}), q=p.toAbsolute(size);
                      return abs instanceof a.Rectangle && abs.left===50 && abs.top===50 && abs.width===25 && abs.height===25 && abs.angle===-90 && abs.fill===null && abs.ordinal===7 && abs.occludeInactive===true && !('ignored' in abs) && normal.width===.125 &&
                        r.absolutePosition(size).left===50 && abs.normalPosition(size).top===.5 &&
                        new a.Rectangle({left:-.005,height:.125}).toAbsolute({width:100,height:900}).height===113 && Object.is(new a.Shape({left:-.005}).toAbsolute({width:100,height:100}).left,-0) &&
                        new a.Ellipse({rx:.25,ry:.5}).toAbsolute(size).rx===50 && new a.Ellipse({rx:50,ry:50}).toNormal(size).ry===.5 &&
                        q.points!==p.points && q.points[0]!==p.points[0] && q.points[0].x===50 && q.points[0].y===50 && q.points[1].x===0 && Number.isNaN(q.points[1].y) && q.toNormal(size).points[0].y===.5 &&
                        [0,NaN,undefined].every(v => new a.Text({fontSize:v}).toAbsolute(size).fontSize===40) && new a.Text({fontSize:.123}).toAbsolute(size).fontSize===12.3 && new a.Text().toNormal(size).fontSize===.4;
                    })()
                    """))
                #expect(try await page.bool("""
                    (() => { let reads=0; const r=new anki.imageOcclusion.Rectangle({get width(){reads++;return .5;}});
                      let fills=0;const t=new anki.imageOcclusion.Text({get fill(){fills++;return '#123456';}});
                      return reads===1 && fills===1 && r.width===.5 && t.fill==='#123456'; })()
                    """))
                try await page.showPresentedQuestion("<canvas id='probe' width='100' height='100'></canvas>")
                #expect(try await page.bool("""
                    (() => {
                      const a=anki.imageOcclusion,c=document.getElementById('probe').getContext('2d');
                      const result=a.drawShape({context:c,size:{width:100,height:100},shape:new a.Rectangle({left:.1,top:.1,width:.5,height:.5}),fill:'#ff0000',stroke:'#000000',strokeWidth:0});
                      return result===undefined && c.getImageData(35,35,1,1).data.join(',')==='255,0,0,255' && c.getImageData(80,80,1,1).data[3]===0;
                    })()
                    """))
                #expect(try await page.bool("(() => { try { anki.imageOcclusion.drawShape(); return false; } catch(e) { return e instanceof TypeError; } })()"))
                #expect(harness.diagnostics.isEmpty)
            }
        }

        /// M4.2/4.3: conversion copies explicit values, rather than applying
        /// construction defaults a second time. All expected geometry is literal.
        @Test func conversionsPreserveExplicitValues() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                let failures = try await page.json([String].self, """
                    const a=anki.imageOcclusion, failures=[], size={width:200,height:100};
                    for(const C of [a.Shape,a.Rectangle,a.Ellipse,a.Polygon,a.Text]) {
                      for(const method of ['toAbsolute','toNormal']) {
                        for(const value of [undefined,null,NaN]) {
                          const absolute=method==='toAbsolute';
                          const source=new C(absolute ?
                            {left:.25,top:.5,width:.125,height:.25,rx:.125,ry:.25,points:[{x:'.25',y:'.5'}],fontSize:.125} :
                            {left:50,top:50,width:25,height:25,rx:25,ry:25,points:[{x:50,y:50}],fontSize:12.5});
                          const keys=['angle','fill','occludeInactive','ordinal'];
                          if(C===a.Text) keys.push('scaleX','scaleY','text');
                          for(const key of keys) source[key]=value;
                          // A custom source id proves conversion generates its
                          // own id even if both constructors share a millisecond.
                          source.id='original-'+C.name;
                          const original=Object.entries(source), result=source[method](size);
                          const tag=C.name+'.'+method+'.'+String(value);
                          for(const key of keys) if(!Object.is(result[key],value)) failures.push(tag+': '+key);
                          if(result===source || result.constructor!==C ||
                            (C===a.Shape ? result.id!==undefined : !/^(rect|ellipse|polygon|text)-[0-9]+$/.test(result.id)) ||
                            result.id===source.id) failures.push(tag+': new instance/id');
                          if(original.some(([key,v])=>!Object.is(source[key],v))) failures.push(tag+': original mutated');
                          if(result.left!==(absolute?50:.25) || result.top!==(absolute?50:.5)) failures.push(tag+': position');
                          if(C===a.Rectangle && (result.width!==(absolute?25:.125) || result.height!==(absolute?25:.25))) failures.push(tag+': rectangle');
                          if(C===a.Ellipse && (result.rx!==(absolute?25:.125) || result.ry!==(absolute?25:.25))) failures.push(tag+': ellipse');
                          if(C===a.Polygon && (result.points===source.points || result.points[0]===source.points[0] ||
                            result.points[0].x!==(absolute?50:.25) || result.points[0].y!==(absolute?50:.5))) failures.push(tag+': polygon');
                          if(C===a.Polygon && (source.points[0].x!==(absolute?'.25':50) || source.points[0].y!==(absolute?'.5':50))) failures.push(tag+': original points mutated');
                          if(C===a.Text && result.fontSize!==(absolute?12.5:.125)) failures.push(tag+': font');
                        }
                      }
                    }
                    // Unchanged properties still have one live getter read per
                    // conversion, and a throwing getter must propagate its error.
                    for(const method of ['toAbsolute','toNormal']) {
                      const s=new a.Text(), counts={}, keys=['angle','fill','occludeInactive','ordinal','scaleX','scaleY','text'];
                      for(const key of keys) {
                        Object.defineProperty(s,key,{get(){counts[key]=(counts[key]||0)+1;return undefined;},configurable:true});
                      }
                      s[method](size);
                      if(keys.some(key=>counts[key]!==1)) failures.push(method+': getter count');
                      const error=new Error('conversion sentinel');
                      Object.defineProperty(s,'fill',{get(){throw error;}});
                      try {s[method](size);failures.push(method+': missing throw');}
                      catch(e) {if(e!==error) failures.push(method+': wrong throw');}
                    }
                    return JSON.stringify(failures);
                    """)
                #expect(failures.isEmpty, "\(failures)")
                try await Self.prepare(harness, page)
                try await page.run("""
                    __ioOptions.onWillDrawShapes=function(d){
                      const a=anki.imageOcclusion;
                      d.activeShapes=[undefined,null,NaN].map(function(scale){
                        const label=new a.Text({left:.2,top:.2,text:'invalid scale',fontSize:.1});
                        label.scaleX=scale;return label;
                      });
                      d.inactiveShapes=[];d.highlightShapes=[];__ioData=d;return d;
                    };
                    """)
                try await Self.showDrawn(page, Self.side())
                #expect(try await page.bool("__ioData.activeShapes[0].scaleX===undefined && __ioData.activeShapes[1].scaleX===null && Number.isNaN(__ioData.activeShapes[2].scaleX)"))
                #expect(try await page.bool("""
                    (()=>{const c=document.getElementById('image-occlusion-canvas');
                      return c.width>0 && c.height>0 && c.getContext('2d').getImageData(0,0,c.width,c.height).data.every((v,i)=>i%4!==3 || v===0);})()
                    """))
                #expect(harness.diagnostics.isEmpty)
            }
        }

        /// Captured numeric and DOM built-ins, but live hook and shape semantics.
        @Test func markupNumericConversions() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await Self.prepare(harness, page)
                let markup = """
                    <span class="cloze" data-shape="rect"></span><p class="cloze" data-shape="rect"></p><div class="cloze" data-shape="RECT"></div>
                    <div class="cloze cloze-inactive" data-shape="rect" data-left=".5px" data-top="0x10" data-angle="-2500" data-width="5px" data-fill="" data-occludeinactive=" 1" data-ordinal="1,2"></div>
                    <div class="cloze" data-shape="rect" data-left="" data-top="  " data-angle="x"></div>
                    <div class="cloze" data-shape="text" data-left="2.5e-1" data-top="abc" data-font-size="" data-text="a&#x3A;b&#x0A;c" data-scale="2px"></div>
                    <div class="cloze" data-shape="polygon" data-points=".1,.2,ignored  .5"></div>
                    <div class="cloze" data-shape="polygon" data-points=""></div>
                    """
                let side = try Self.fixture("page")
                    .replacingOccurrences(of: "{{shapes}}", with: markup)
                    .replacingOccurrences(of: "{{button}}", with: "")
                try await page.run("""
                    __ioOptions.onWillDrawShapes=function(d) { __ioData=d; __ioPasses++; return {activeShapes:[],inactiveShapes:[],highlightShapes:[],properties:d.properties}; };
                    // Save native functions for assertions after the card runs.
                    window.__ioRestore = [Number,parseFloat,parseInt,Math.round,Document.prototype.querySelectorAll,CanvasRenderingContext2D.prototype.fillRect];
                    Number=function(){throw Error('late Number');}; parseFloat=function(){throw Error('late parseFloat');}; parseInt=function(){throw Error('late parseInt');};
                    Math.round=function(){throw Error('late round');}; Document.prototype.querySelectorAll=function(){throw Error('late query');};
                    CanvasRenderingContext2D.prototype.fillRect=function(){throw Error('late canvas');};
                    """)
                try await Self.showDrawn(page, side)
                #expect(try await page.bool("""
                    (() => {const c=document.createElement('canvas').getContext('2d');
                      anki.imageOcclusion.drawShape({context:c,size:{width:100,height:100},shape:new anki.imageOcclusion.Rectangle({width:.5,height:.5}),fill:'#ff0000',stroke:'#000000',strokeWidth:0});
                      return c.getImageData(20,20,1,1).data.join(',')==='255,0,0,255';})()
                    """))
                try await page.run("[Number,parseFloat,parseInt,Math.round,Document.prototype.querySelectorAll,CanvasRenderingContext2D.prototype.fillRect]=__ioRestore;")
                #expect(try await page.bool("""
                    (() => { const s=__ioData.activeShapes; return s.length===5 && __ioData.inactiveShapes.length===1 &&
                      s[0].left===0 && s[0].top===0 && s[0].angle===-90 && s[0].width===5 && Number.isNaN(s[0].height) && s[0].fill==='' && !s[0].occludeInactive && s[0].ordinal===1 &&
                      Number.isNaN(s[1].left) && Number.isNaN(s[1].top) && s[1].angle===0 &&
                      s[2].left===.25 && s[2].top===0 && s[2].fontSize===undefined && Array.from(s[2].text,c=>c.charCodeAt(0)).join(',')==='97,58,98,10,99' && s[2].scaleX===2 &&
                      s[3].points.length===3 && s[3].points[0].x==='.1' && s[3].points[0].y==='.2' && s[3].points[1].x==='' && s[3].points[1].y===0 && s[3].points[2].x==='.5' && s[3].points[2].y===0 &&
                      s[4].points[0].x===0 && s[4].points[0].y===0; })()
                    """))
                #expect(harness.diagnostics.isEmpty)
            }
        }

        /// HK1–4/HK7: order, context, receiver, replacement, mutation, and
        /// promise completion before any draw. Hooks are read at each pass.
        @Test func hooksAndAsyncSetup() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await Self.prepare(harness, page)
                try await page.run("""
                    __ioOptions = {
                      onWillDrawShapes: function(d,c) { 'use strict';
                        log(['will',d.activeShapes.length,d.inactiveShapes.length,d.highlightShapes.length,d.properties.activeShapeColor,d.properties.inActiveBorder.width,c===document.getElementById('image-occlusion-canvas').getContext('2d'),this===undefined].join(','));
                        __ioData=d; __ioPasses++;
                      },
                      onDidDrawShapes: function(d,c) { 'use strict'; log('did:'+(__ioData.activeShapes===d.activeShapes)+':'+(this===undefined)); }
                    };
                    """)
                try await Self.showDrawn(page, Self.side())
                #expect(try await page.log() == ["will,2,2,0,#ff0000,0,true,true", "did:true:true"])
                #expect(try await page.string("__ioResolved.join(',')") == "0")
                #expect(try await page.bool("__ioData.activeShapes[0] instanceof anki.imageOcclusion.Rectangle && __ioData.activeShapes[1] instanceof anki.imageOcclusion.Text && __ioData.inactiveShapes[0].left===.05"))
                try await page.run("""
                    __ioOptions.onWillDrawShapes = function(d,c) {
                      __ioPasses++; __ioData=d;
                      const p=Object.assign({},d.properties,{inActiveShapeColor:'#00ffff'});
                      return {activeShapes:[],inactiveShapes:d.inactiveShapes,highlightShapes:[],properties:p};
                    };
                    __ioOptions.onDidDrawShapes = function(d,c) { log('returned:'+d.activeShapes.length+':'+d.properties.inActiveShapeColor); };
                    """)
                try await Self.showDrawn(page, Self.side(answer: true), answer: true)
                #expect(try await Self.pixel(page, ".15*W", ".175*H") == [0,255,255,255])
                #expect(try await Self.pixel(page, ".5*W", ".175*H") == Self.clear)
                #expect(try await Self.pixel(page, "Math.round(.05*W)+2", "Math.round(.6*H)+2") == Self.clear)
                #expect(try await page.log().last == "returned:0:#00ffff")
                try await page.run("""
                    __ioOptions.onWillDrawShapes = function(d) { __ioPasses++; d.activeShapes.length=0; d.highlightShapes.length=0; };
                    __ioOptions.onDidDrawShapes = null;
                    document.getElementById('toggle').click(); document.getElementById('toggle').click();
                    """)
                #expect(try await Self.pixel(page, ".15*W", ".175*H") == Self.blue)
                #expect(try await Self.pixel(page, "Math.round(.05*W)+2", "Math.round(.6*H)+2") == Self.clear)
                // Return getters are read once, and did receives the exact values drawn.
                try await page.run("""
                    __log=[];
                    __ioOptions.onWillDrawShapes = function(d) {
                      let reads=0; const active=d.activeShapes;
                      __ioOptions.onDidDrawShapes=function(after){log('reads:'+reads+':same:'+(after.activeShapes===active));};
                      return {get activeShapes(){reads++;return active;},inactiveShapes:d.inactiveShapes,highlightShapes:d.highlightShapes,properties:d.properties};
                    };
                    """)
                try await Self.showDrawn(page, Self.side())
                #expect(try await page.log() == ["reads:1:same:true"])
                // A present setup promise is just scheduling, including null/non-object options.
                for value in ["5", "null", "undefined"] {
                    try await page.showPresentedQuestion("<p>no canvas</p>")
                    #expect(try await page.bool("(await anki.imageOcclusion.setup(\(value))) === undefined"))
                    try await Self.frames(page)
                    #expect(try await page.string("document.getElementById('qa').textContent") == "no canvas")
                }
                #expect(harness.diagnostics.isEmpty)
            }
        }

        /// M5.1: each pending call waits for its original image even after
        /// removal; fulfilment follows scheduling and precedes the no-op step.
        @Test func detachedSetupStillWaitsForOriginalImage() async throws {
            let harness = WebTestHarness()
            defer { harness.close() }
            Self.instrument(harness, source: """
                window.__ioSetupTrace=[]; window.__ioCaptureSetupFrames=false; window.__ioSetupFramesScheduled=0;
                const frame=window.requestAnimationFrame;
                window.requestAnimationFrame=function(callback){
                  if(!__ioCaptureSetupFrames) return Reflect.apply(frame,window,[callback]);
                  const n=++__ioSetupFramesScheduled;
                  __ioSetupTrace.push('scheduled:'+n);
                  return Reflect.apply(frame,window,[function(time){
                    __ioSetupTrace.push('frame:'+n);
                    return Reflect.apply(callback,window,[time]);
                  }]);
                };
                """)
            let page = try await harness.openPage()
            try await Self.prepare(harness, page)
            harness.media.delay("/_io.png", by: .milliseconds(300))
            try await page.showPresentedQuestion("<p>before pending setup</p>")
            // Create the image directly, bypassing side preload. Removal and
            // both setup calls happen in this task, before any image event.
            let completeBeforeSetup = try #require(try await page.run("""
                const qa=document.getElementById('qa');
                qa.innerHTML='<div id="image-occlusion-container"><canvas id="image-occlusion-canvas"></canvas></div>';
                const img=window.__ioOriginalImage=document.createElement('img');
                document.getElementById('image-occlusion-container').prepend(img);
                function originalReady(event){
                  window.__ioOriginalEventTrusted=event.isTrusted;
                  __ioSetupTrace.push(event.type);
                  // Capture only scheduling in this image event's listeners.
                  __ioCaptureSetupFrames=true;
                }
                img.addEventListener('load',originalReady,{once:true});
                img.addEventListener('error',originalReady,{once:true});
                img.src='_io.png';
                const completeBeforeSetup=img.complete;
                if(completeBeforeSetup!==false) throw Error('original image must be incomplete before setup');
                const options={onWillDrawShapes:function(){__ioSetupTrace.push('unexpected draw');}};
                for(let n=1;n<=2;n++){
                  __ioSetupTrace.push('call:'+n);
                  anki.imageOcclusion.setup(options).then(function(value){
                    __ioSetupTrace.push('resolved:'+n+':'+String(value));
                  });
                }
                function endReady(){__ioCaptureSetupFrames=false;}
                img.addEventListener('load',endReady,{once:true});
                img.addEventListener('error',endReady,{once:true});
                qa.replaceChildren(document.createTextNode('next side'));
                __ioSetupTrace.push('removed');
                return completeBeforeSetup;
                """) as? Bool)
            try #require(!completeBeforeSetup)
            try await harness.until("original image event and both setup fulfilments") {
                try await page.bool("""
                    (__ioSetupTrace.includes('load') || __ioSetupTrace.includes('error')) &&
                    __ioSetupTrace.includes('resolved:1:undefined') && __ioSetupTrace.includes('resolved:2:undefined')
                    """)
            }
            try await Self.frames(page)
            let trace = try await page.json([String].self, "return JSON.stringify(__ioSetupTrace);")
            #expect(Array(trace.prefix(3)) == ["call:1", "call:2", "removed"])
            #expect(trace.count == 10, "setup trace: \(trace)")
            let eventIndex = try #require(trace.firstIndex(of: "load"))
            // Engines may checkpoint microtasks between event listeners. Each
            // call must still schedule before its fulfilment and later frame.
            for n in 1...2 {
                let resolved = try #require(trace.firstIndex(of: "resolved:\(n):undefined"), "setup trace: \(trace)")
                #expect(eventIndex < resolved, "image event must precede setup result: \(trace)")
                let scheduled = try #require(trace.firstIndex(of: "scheduled:\(n)"), "setup trace: \(trace)")
                let frame = try #require(trace.firstIndex(of: "frame:\(n)"), "setup trace: \(trace)")
                #expect(eventIndex < scheduled && scheduled < resolved && resolved < frame, "setup trace: \(trace)")
            }
            #expect(try await page.bool("__ioOriginalEventTrusted && __ioOriginalImage.complete && __ioOriginalImage.naturalWidth===400 && !__ioOriginalImage.isConnected"))
            #expect(try await page.string("document.getElementById('qa').textContent") == "next side")
            #expect(harness.diagnostics.isEmpty)
        }

        /// ER1–6 and M9 error routes, with real native identity and no sync
        /// catch masquerading as observation of the later rejection.
        @Test func failuresRemainAsyncAndTemplateOwnsError() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await Self.prepare(harness, page)
                let missing = try Self.side().replacingOccurrences(of: "<img src=\"_io.png\">", with: "")
                    .replacingOccurrences(of: "<div id=\"err\"></div>", with: "<div id=\"err\">template sentinel</div>")
                try await page.announce(card: 680, generation: 1)
                try await page.showPresentedQuestion(missing)
                try await harness.until("final missing-image DOM") {
                    try await page.bool("document.getElementById('image-occlusion-container').innerText==='No image to show.' && document.querySelector('canvas')===null")
                }
                #expect(try await page.string("document.getElementById('image-occlusion-container').innerText") == "No image to show.")
                #expect(try await page.bool("document.querySelector('canvas')===null"))
                #expect(try await page.string("document.getElementById('err').textContent") == "template sentinel")
                #expect(harness.diagnostics.isEmpty)
                let broken = try Self.side().replacingOccurrences(of: "_io.png", with: "_missing.png")
                    .replacingOccurrences(of: "<div id=\"err\"></div>", with: "<div id=\"err\">template sentinel</div>")
                try await page.announce(card: 680, generation: 2)
                try await Self.showDrawn(page, broken)
                #expect(try await page.bool("document.getElementById('image-occlusion-canvas').width===0 && document.getElementById('image-occlusion-canvas').height===0"))
                #expect(try await page.string("document.getElementById('image-occlusion-container').style.aspectRatio") == "")
                #expect(try await page.string("document.getElementById('err').textContent") == "template sentinel")
                #expect(harness.diagnostics.isEmpty)
                try await harness.until("missing image resource diagnostic") { !harness.signals("mediaError").isEmpty }

                let cases: [(String, String, Bool)] = [
                    (try Self.side(8), "TypeError", false),
                    (try Self.side(), "Error: x", false),
                    (try Self.side(), "TypeError", false),
                    (try Self.side(), "Error: after", true),
                    (try Self.side(), "TypeError", true),
                    (try Self.side().replacingOccurrences(of: "data-shape=\"rect\"", with: "data-shape=\"ellipse\" data-rx=\"-.1\" data-ry=\".1\""), "IndexSizeError", false),
                    (try Self.side().replacingOccurrences(of: "data-ordinal=\"3\" data-shape=\"rect\"", with: "data-ordinal=\"3\" data-shape=\"ellipse\" data-rx=\"-.1\" data-ry=\".1\""), "IndexSizeError", true),
                ]
                for (index, test) in cases.enumerated() {
                    try await page.run("""
                        __ioOptions = {onWillDrawShapes: function(d) { __ioPasses++; }};
                        if (n===1) __ioOptions.onWillDrawShapes=function(){throw Error('x');};
                        if (n===2) __ioOptions.onWillDrawShapes=7;
                        if (n===3) __ioOptions.onDidDrawShapes=function(){throw Error('after');};
                        if (n===4) __ioOptions.onDidDrawShapes=7;
                        """, ["n": index])
                    let before = harness.diagnostics.count
                    let beforePasses = try await page.string("__ioPasses")
                    let resolutions = try #require(try await page.run("return __ioResolved.length;") as? Int)
                    try await page.announce(card: 681, generation: Int64(index + 3))
                    try await page.showPresentedQuestion(test.0)
                    try await harness.until("IO rejection \(index)") {
                        harness.diagnostics.count == before + 1 && harness.diagnostics.last?.card == 681 &&
                            harness.diagnostics.last?.generation == Int64(index + 3) && harness.diagnostics.last?.side == "question"
                    }
                    let error = try #require(harness.diagnostics.last)
                    #expect(error["kind"] as? String == "rejection")
                    if index == 0 { #expect(try await page.string("__ioPasses") == beforePasses) }
                    #expect((error["message"] as? String)?.hasPrefix(test.1) == true)
                    #expect(error.side == "question" && error.card == 681 && error.generation == Int64(index + 3))
                    #expect(try await page.string("document.getElementById('err').textContent") == "")
                    #expect(try await page.run("return __ioResolved.length;") as? Int == resolutions + 1)
                    #expect(try await Self.pixel(page, ".5*W", ".175*H") == (test.2 ? Self.red : Self.clear))
                }
                try await page.run("__ioOptions={onWillDrawShapes: function(){__ioPasses++;}};")
                try await Self.showDrawn(page, Self.side(answer: true), answer: true)
                try await page.run("__ioOptions.onWillDrawShapes=function(){throw Error('click');}; document.getElementById('toggle').click();")
                try await harness.until("click exception") { harness.diagnostics.contains { ($0["message"] as? String) == "Error: click" } }
                #expect(harness.diagnostics.last?["kind"] as? String == "exception")
                #expect(harness.diagnostics.last?["phase"] as? String == "later")
                let count = harness.diagnostics.count
                try await page.run("document.getElementById('image-occlusion-canvas').remove(); window.dispatchEvent(new KeyboardEvent('keydown',{key:'M'}));")
                try await harness.until("key with no canvas") { harness.diagnostics.count > count }
                #expect(harness.diagnostics.last?["kind"] as? String == "exception")
                #expect((harness.diagnostics.last?["message"] as? String)?.hasPrefix("TypeError") == true)

                let before = harness.diagnostics.count
                try await page.showPresentedQuestion("<canvas id='image-occlusion-canvas'></canvas><script>anki.imageOcclusion.setup()</script>")
                try await harness.until("no container rejection") { harness.diagnostics.count > before }
                #expect((harness.diagnostics.last?["message"] as? String)?.hasPrefix("TypeError") == true)
            }
        }

        /// M9/M11 and media-serving M5/R498: retry the real failed image via
        /// the isolated native seam, preserving the side and its current state.
        @Test func failedImageRecoversAfterMediaArrival() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await Self.prepare(harness, page)
                let broken = try Self.side(answer: true)
                    .replacingOccurrences(of: "_io.png", with: "_arrival.png")
                    .replacingOccurrences(of: "<div id=\"err\"></div>", with: "<div id=\"err\">template sentinel</div><input id=\"retained\">")
                    + "<script>window.__ioScriptRuns=(window.__ioScriptRuns||0)+1;</script>"
                try await page.announce(card: 682, generation: 1)
                try await Self.showDrawn(page, broken)
                try await harness.until("initial IO image 404") {
                    harness.signals("mediaError").contains { ($0["url"] as? String)?.contains("_arrival.png") == true }
                }
                #expect(try await page.bool("""
                    (()=>{const i=document.querySelector('#image-occlusion-container img'),c=document.getElementById('image-occlusion-canvas');
                      return i.complete && i.naturalWidth===0 && c.width===0 && c.height===0 &&
                        document.getElementById('image-occlusion-container').style.aspectRatio==='' && __ioResolved.length===1;})()
                    """))
                #expect(harness.diagnostics.isEmpty)
                try await page.run("""
                    document.getElementById('retained').value='keep my answer';
                    window.__ioRetained={nodes:Array.from(document.querySelectorAll('#qa *')),image:document.querySelector('#image-occlusion-container img'),
                      canvas:document.getElementById('image-occlusion-canvas'),container:document.getElementById('image-occlusion-container'),options:__ioOptions};
                    document.getElementById('toggle').click();
                    // Mutate the current options after setup and the hidden-state
                    // toggle. Recovery must read this hook without calling setup.
                    __ioOptions.onWillDrawShapes=function(d){
                      __ioPasses++;__ioData=d;log('arrival latest');
                      for(const s of d.activeShapes) if(s instanceof anki.imageOcclusion.Text) s.text='latest';
                      return d;
                    };
                    __log=[];
                    """)
                let draws = try #require(try await page.run("return __ioDraws;") as? Int)
                let passes = try #require(try await page.run("return __ioPasses;") as? Int)
                harness.media.add("/_arrival.png", TestMedia.png(width: 640, height: 320), "image/png")
                try await page.runIsolated("meguriCard.mediaArrived(['_arrival.png']);")
                try await harness.until("retried IO image has natural size") {
                    try await page.bool("__ioRetained.image.naturalWidth===640 && __ioRetained.image.naturalHeight===320")
                }
                try await harness.until("IO recovery draw after media arrival") { try await page.bool("__ioDraws > \(draws)") }
                try await Self.frames(page)
                #expect(try await page.bool("""
                    parseFloat(__ioRetained.container.style.aspectRatio)===2 && __ioRetained.container.clientWidth===400 &&
                    __ioRetained.container.clientHeight===200 && __ioRetained.canvas.clientWidth===400 && __ioRetained.canvas.clientHeight===200
                    """))
                try await Self.expectCurrentSize(page)
                #expect(try await Self.pixel(page, ".15*W", ".175*H") == Self.clear)
                #expect(try await Self.pixel(page, ".5*W", ".175*H") == Self.clear)
                #expect(try await Self.pixel(page, "Math.round(.05*W)+2", "Math.round(.6*H)+2") == Self.white)
                #expect(try await page.bool("""
                    __ioData.activeShapes.length===1 && __ioData.activeShapes[0].text==='latest' &&
                    __ioData.inactiveShapes.length===0 && __ioData.highlightShapes.length===0 &&
                    __ioOptions===__ioRetained.options && __ioResolved.length===1 && __ioScriptRuns===1 &&
                    document.getElementById('retained').value==='keep my answer' && document.getElementById('err').textContent==='template sentinel' &&
                    document.querySelector('#image-occlusion-container img')===__ioRetained.image &&
                    document.getElementById('image-occlusion-canvas')===__ioRetained.canvas &&
                    document.getElementById('image-occlusion-container')===__ioRetained.container &&
                    Array.from(document.querySelectorAll('#qa *')).every((node,index)=>node===__ioRetained.nodes[index]) &&
                    document.querySelectorAll('#qa *').length===__ioRetained.nodes.length
                    """))
                #expect(try await page.log() == ["arrival latest"])
                #expect(try await page.run("return __ioPasses;") as? Int == passes + 1)
                #expect(try await page.run("return __ioDraws;") as? Int == draws + 1)
                #expect(harness.diagnostics.isEmpty)
                // An unchanged successful load is a no-op; an obsolete image
                // cannot redraw a later side or produce an IO error.
                try await page.run("__ioRetained.image.dispatchEvent(new Event('load'));")
                try await Self.frames(page)
                #expect(try await page.run("return __ioDraws;") as? Int == draws + 1)
                try await page.showPresentedQuestion("<p>next side</p>")
                try await page.run("__ioRetained.image.dispatchEvent(new Event('load')); window.dispatchEvent(new Event('resize'));")
                try await Self.frames(page)
                #expect(try await page.run("return __ioDraws;") as? Int == draws + 1)
                #expect(try await page.string("document.getElementById('qa').textContent") == "next side")
                #expect(harness.diagnostics.isEmpty)
            }
        }

        /// HK5/6, TG7, D2/3 and per-button independence after side/card replacement.
        @Test func repeatedSetupAndRealSideLifecycle() async throws {
            let harness = WebTestHarness()
            defer { harness.close() }
            Self.instrument(harness, source: """
                window.__ioKeyAdds=0;
                const original=EventTarget.prototype.addEventListener;
                EventTarget.prototype.addEventListener=function(type,fn,options) {
                  if (this===window && type==='keydown') __ioKeyAdds++;
                  return Reflect.apply(original,this,[type,fn,options]);
                };
                """)
            let page = try await harness.openPage()
            try await page.installLog()
            try await Self.prepare(harness, page)
            let keysBefore = try #require(try await page.run("return __ioKeyAdds;") as? Int)
            try await page.announce(card: 1, generation: 1)
            try await Self.showDrawn(page, Self.side())
            try await Self.showDrawn(page, Self.side(answer: true), answer: true)
            try await page.run("document.getElementById('toggle').click(); window.__ioOldButton=document.getElementById('toggle');")
            #expect(try await Self.pixel(page, ".15*W", ".175*H") == Self.clear)
            try await page.run("__ioOptions={onWillDrawShapes: function(){__ioPasses++; log('B');}};")
            try await page.announce(card: 2, generation: 2)
            try await Self.showDrawn(page, Self.side())
            try await Self.showDrawn(page, Self.side(answer: true), answer: true)
            #expect(try await Self.pixel(page, ".15*W", ".175*H") == Self.blue)
            try await page.run("document.getElementById('toggle').click();")
            #expect(try await Self.pixel(page, ".15*W", ".175*H") == Self.clear)
            let before = try #require(try await page.run("return __ioPasses;") as? Int)
            #expect(try await page.bool("(() => {const e=new KeyboardEvent('keydown',{key:'M',cancelable:true,ctrlKey:true}); return window.dispatchEvent(e) && !e.defaultPrevented;})()"))
            #expect(try await page.run("return __ioPasses;") as? Int == before + 1)
            #expect(try await page.log().last == "B")
            #expect(try await page.run("return __ioKeyAdds;") as? Int == keysBefore + 1)
            try await page.run("document.getElementById('image-occlusion-container').style.width='420px'; window.dispatchEvent(new Event('resize'));")
            try await Self.passes(page, before + 2)
            #expect(try await page.log().last == "B")
            // Old buttons retain their own setup options and still read the current DOM.
            try await page.run("__log=[]; __ioOldButton.click(); __ioOldButton=null;")
            #expect(try await page.log().isEmpty)
            #expect(try await Self.pixel(page, ".15*W", ".175*H") == Self.clear)

            // Two explicit calls mean two frames/passes and two new click listeners.
            // Replace the side first so this button has exactly those two listeners.
            let noAutoSetup = try Self.side(answer: true).replacingOccurrences(of: "anki.imageOcclusion.setup(window.__ioOptions)", with: "Promise.resolve()")
            try await page.showPresentedAnswer(noAutoSetup)
            try await page.run("""
                __log=[];
                anki.imageOcclusion.setup({onWillDrawShapes:function(){log('first');}});
                anki.imageOcclusion.setup({onWillDrawShapes:function(){log('second');}});
                """)
            try await harness.until("both independent setups") { try await page.log().count == 2 }
            #expect(try await page.log() == ["first", "second"])
            try await page.run("__log=[]; document.getElementById('toggle').click();")
            #expect(try await page.log() == ["first", "second"])
            #expect(try await Self.pixel(page, ".15*W", ".175*H") == Self.blue)
            try await page.run("__log=[]; window.dispatchEvent(new KeyboardEvent('keydown',{key:'M'}));")
            #expect(try await page.log() == ["second"])
            #expect(try await Self.pixel(page, ".15*W", ".175*H") == Self.clear)
            #expect(harness.diagnostics.isEmpty)
        }

        /// SZ1–5, including D5, with DPR controlled *before* built-in capture.
        @Test func backingDimensionsStayBounded() async throws {
            let harness = WebTestHarness()
            defer { harness.close() }
            Self.instrument(harness, source: "window.__ioDPR=1; Object.defineProperty(window,'devicePixelRatio',{get:function(){return __ioDPR;},configurable:true});")
            let page = try await harness.openPage(size: CGSize(width: 2000, height: 2400))
            try await Self.prepare(harness, page)
            let cases = [(400,300,400,1,400,300), (400,300,400,2,800,600), (400,300,400,3,1200,900),
                         (3000,2000,1800,1,1800,1200), (3000,2000,1800,2,3600,2400), (3000,2000,1800,3,4096,2730),
                         (8000,1000,1600,3,4096,512), (1000,1000,1400,3,4096,4096)]
            for (index, test) in cases.enumerated() {
                let (nw,nh,width,dpr,w,h) = test
                harness.media.add("/_size\(index).png", TestMedia.png(width: nw, height: nh), "image/png")
                try await page.run("__ioDPR=d;", ["d": dpr])
                let side = try Self.side().replacingOccurrences(of: "width: 400px", with: "width: \(width)px").replacingOccurrences(of: "_io.png", with: "_size\(index).png")
                try await Self.showDrawn(page, side)
                #expect(try await page.bool("document.getElementById('image-occlusion-canvas').width===\(w) && document.getElementById('image-occlusion-canvas').height===\(h)"))
                let actualRatio = try await page.string("document.getElementById('image-occlusion-container').style.aspectRatio")
                let expectedRatio = try #require(try await page.run("""
                    const expected=document.createElement('div');
                    expected.style.aspectRatio=String(\(nw)/\(nh));
                    return expected.style.aspectRatio;
                    """) as? String)
                #expect(actualRatio == expectedRatio, "aspect-ratio for \(nw)x\(nh): actual \(actualRatio), expected \(expectedRatio)")
                #expect(try await page.bool("document.getElementById('image-occlusion-container').clientHeight===\(Int(Double(width)*Double(nh)/Double(nw)))"))
            }
            // Integer client-height limiting, with no independent axis stretch.
            harness.media.add("/_limit.png", TestMedia.png(width: 1600, height: 1200), "image/png")
            for (dpr,w,h) in [(2,698,524),(3,1048,786)] {
                try await page.run("__ioDPR=d;", ["d": dpr])
                let side = try Self.side().replacingOccurrences(of: "width: 400px", with: "width:350px; height:262px").replacingOccurrences(of: "_io.png", with: "_limit.png")
                try await Self.showDrawn(page, side)
                #expect(try await page.bool("document.getElementById('image-occlusion-canvas').width===\(w) && document.getElementById('image-occlusion-canvas').height===\(h)"))
            }
            #expect(harness.diagnostics.isEmpty)
        }

        /// RD1–6, D1: actual frame resize, actual page zoom, DPR-only change,
        /// coalescing, no-op events and release of the detached observation.
        @Test func redrawOnResizeZoomDPRAndCleanup() async throws {
            let harness = WebTestHarness()
            defer { harness.close() }
            Self.instrument(harness, source: """
                window.__ioDPR=1; window.__ioMediaQueries=[]; window.__ioDisconnected=0;
                Object.defineProperty(window,'devicePixelRatio',{get:function(){return __ioDPR;},configurable:true});
                const match=window.matchMedia;
                window.matchMedia=function(q){const m=Reflect.apply(match,window,[q]); __ioMediaQueries.push(m); return m;};
                const nativeStop=ResizeObserver.prototype.disconnect;
                ResizeObserver.prototype.disconnect=function(){__ioDisconnected++; return Reflect.apply(nativeStop,this,[]);};
                """)
            let page = try await harness.openPage(size: CGSize(width: 390, height: 600))
            try await Self.prepare(harness, page)
            let auto = try Self.side().replacingOccurrences(of: "width: 400px", with: "width: auto")
            try await Self.showDrawn(page, auto)
            #expect(try await page.string("__ioPasses") == "1")
            page.webView.frame.size.width = 700
            try await Self.passes(page, 2)
            try await Self.frames(page)
            #expect(try await page.string("__ioPasses") == "2")
            try await Self.expectCurrentSize(page)
            #expect(try await Self.pixel(page, ".5*W", ".175*H") == Self.red)
            page.webView.pageZoom = 1.5
            try await Self.passes(page, 3)
            try await Self.frames(page)
            #expect(try await page.string("__ioPasses") == "3")
            try await Self.expectCurrentSize(page)
            try await page.run("__ioDPR=3; __ioMediaQueries[__ioMediaQueries.length-1].dispatchEvent(new Event('change'));")
            try await Self.passes(page, 4)
            try await Self.frames(page)
            #expect(try await page.string("__ioPasses") == "4")
            try await Self.expectCurrentSize(page)
            try await page.run("for(let i=0;i<20;i++) window.dispatchEvent(new Event('resize'));")
            try await Self.frames(page)
            #expect(try await page.string("__ioPasses") == "4")
            try await page.run("const container=document.getElementById('image-occlusion-container'); container.style.width='310px'; for(let i=0;i<20;i++) window.dispatchEvent(new Event('resize'));")
            try await Self.passes(page, 5)
            try await Self.frames(page)
            #expect(try await page.string("__ioPasses") == "5")
            try await Self.expectCurrentSize(page)
            // Appearance alone has no draw trigger.
            try await page.runIsolated("meguriCard.setAppearance(true, true);")
            try await Self.frames(page)
            #expect(try await page.string("__ioPasses") == "5")
            let answer = try Self.side(answer: true).replacingOccurrences(of: "width: 400px", with: "width: auto")
            try await Self.showDrawn(page, answer, answer: true)
            try await page.run("document.getElementById('toggle').click();")
            #expect(try await Self.pixel(page, ".15*W", ".175*H") == Self.clear)
            for width in [650,600,550] {
                let before = try #require(try await page.run("return __ioPasses;") as? Int)
                page.webView.frame.size.width = CGFloat(width)
                try await Self.passes(page, before + 1)
                try await Self.frames(page)
                #expect(try await page.run("return __ioPasses;") as? Int == before + 1)
                #expect(try await Self.pixel(page, ".15*W", ".175*H") == Self.clear)
                try await Self.expectCurrentSize(page)
            }
            let before = try #require(try await page.run("return __ioPasses;") as? Int)
            try await page.run("document.getElementById('toggle').click();")
            #expect(try await page.run("return __ioPasses;") as? Int == before + 1)
            #expect(try await Self.pixel(page, ".15*W", ".175*H") == Self.blue)
            let disconnected = try #require(try await page.run("return __ioDisconnected;") as? Int)
            try await page.showPresentedQuestion("<p>plain next card</p>")
            try await page.run("window.dispatchEvent(new Event('resize'));")
            page.webView.frame.size.width = 500
            try await Self.frames(page)
            #expect(try await page.run("return __ioDisconnected;") as? Int == disconnected + 1)
            #expect(try await page.run("return __ioPasses;") as? Int == before + 1)
            #expect(harness.diagnostics.isEmpty)
        }

        /// M6 expected arithmetic, using only DOM measurements as inputs.
        static func expectCurrentSize(_ page: TestPage) async throws {
            struct Size: Decodable { let cw, ch, nw, nh, d, w, h: Double }
            let s = try await page.json(Size.self, """
                const c=document.getElementById('image-occlusion-canvas'),i=document.querySelector('#image-occlusion-container img');
                return JSON.stringify({cw:c.clientWidth,ch:c.clientHeight,nw:i.naturalWidth,nh:i.naturalHeight,d:devicePixelRatio,w:c.width,h:c.height});
                """)
            let scale = min(s.cw * s.d / s.nw, s.ch * s.d / s.nh)
            var w = s.nw * scale
            var h = s.nh * scale
            if max(w,h) > 4096 {
                let reduction = 4096 / max(w,h)
                w *= reduction
                h *= reduction
            }
            #expect(s.w == floor(w) && s.h == floor(h) && s.w <= 4096 && s.h <= 4096)
        }

        /// M7.2/5: CSS parsing is textual, hook mutations of context survive
        /// drawing/toggle, and changing a hook's did member is observed live.
        @Test func bordersAndLiveContext() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await Self.prepare(harness, page)
                try await page.run("""
                    __ioOptions = {
                      onWillDrawShapes: function(d,c) {
                        __ioData=d; __ioPasses++;
                        c.translate(1,2); c.globalAlpha=.5;
                        __ioOptions.onDidDrawShapes=function(after,context){
                          log([context.getTransform().e,context.getTransform().f,context.globalAlpha,after.properties.activeBorder.width,after.properties.activeBorder.color].join(','));
                        };
                      }
                    };
                    """)
                let side = try Self.side(answer: true)
                    .replacingOccurrences(of: "0px #000000", with: "2px #00ff00")
                    .replacingOccurrences(of: "1px #00ff00", with: "1px solid red")
                try await Self.showDrawn(page, side)
                #expect(try await page.log() == ["1,2,0.5,2,#00ff00"])
                #expect(try await page.bool("__ioData.properties.highlightShapeBorder.width===1 && __ioData.properties.highlightShapeBorder.color==='solid'"))
                try await page.run("document.getElementById('toggle').click();")
                #expect(try await page.log().last == "2,4,0.5,2,#00ff00")
                #expect(try await page.bool("document.getElementById('image-occlusion-canvas').getContext('2d').getTransform().e===2"))
                let borders = try Self.side()
                    .replacingOccurrences(of: "--active-shape-border: 0px #000000", with: "--active-shape-border: #212121 1px")
                    .replacingOccurrences(of: "--inactive-shape-border: 0px #000000", with: "--inactive-shape-border: 2px")
                try await Self.showDrawn(page, borders)
                #expect(try await page.bool("__ioData.properties.activeBorder.width===1 && __ioData.properties.activeBorder.color==='1px' && __ioData.properties.inActiveBorder.width===2 && __ioData.properties.inActiveBorder.color==='#212121'"))
                #expect(harness.diagnostics.isEmpty)
            }
        }

        /// AP7: eligible delayed image, synchronous script walk, unpresented.
        /// No IO barrier or budget change is used to obtain this ordering.
        @Test func delayedImageDrawsBeforeExistingSettled() async throws {
            try await WebTestHarness.withPage(frameClock: true) { harness, page async throws in
                try await Self.prepare(harness, page)
                harness.media.delay("/_io.png", by: .milliseconds(300))
                try await page.announce(card: 68, generation: 4)
                try await page.showQuestion(Self.side())
                let settled = try await harness.signal("settled", from: page)
                #expect(settled.card == 68 && settled.generation == 4)
                #expect(try await Self.pixel(page, ".5*W", ".175*H") == Self.red)
                #expect(try await page.string("__ioResolved.join(',')") == "0")
                #expect(try await page.string("__ioPasses") == "1")
                #expect(harness.signals("shown").isEmpty)
                #expect(harness.diagnostics.isEmpty)
            }
        }

        static func instrument(_ harness: WebTestHarness, source: String) {
            harness.configuration.userContentController.addUserScript(WKUserScript(
                source: "(function(){\(source)})();", injectionTime: .atDocumentStart, forMainFrameOnly: true, in: .page))
        }

        /// GE5/6: record before production captures built-ins; compare metrics
        /// from an independent context, plus actual white-box/transparent pixels.
        @Test func labelLayoutAndContextState() async throws {
            let harness = WebTestHarness()
            defer { harness.close() }
            Self.instrument(harness, source: """
                window.__ioCalls=[];
                const p=CanvasRenderingContext2D.prototype;
                ['fillRect','fillText','scale','rotate','translate','strokeRect','clearRect'].forEach(function(k){
                  const original=p[k];
                  p[k]=function(...args){__ioCalls.push({method:k,args:args,font:this.font,fill:this.fillStyle,matrix:Array.from([this.getTransform().a,this.getTransform().b,this.getTransform().c,this.getTransform().d,this.getTransform().e,this.getTransform().f])});return Reflect.apply(original,this,args);};
                });
                """)
            let page = try await harness.openPage()
            try await Self.prepare(harness, page)
            try await Self.showDrawn(page, Self.side(4))
            #expect(try await page.bool("""
                (() => {
                  const canvas=document.getElementById('image-occlusion-canvas'),W=canvas.width,H=canvas.height;
                  const texts=__ioCalls.filter(x=>x.method==='fillText');
                  const label=texts.find(x=>x.args[0]==='Label'),plain=texts.find(x=>x.args[0]==='Plain'),a=texts.find(x=>x.args[0]==='a:b'),b=texts.find(x=>x.args[0]==='c');
                  const probe=document.createElement('canvas').getContext('2d'); probe.font=(.05*H)+'px Arial'; probe.textBaseline='top';
                  const m=probe.measureText('M'),L=1.5*(m.actualBoundingBoxAscent+m.actualBoundingBoxDescent);
                  return label.font===(.1*H)+'px Arial' && label.args[1]===Math.round(.1*W) && label.args[2]===Math.round(.1*H) && label.matrix.join(',')==='1,0,0,1,0,0' &&
                    plain.font==='40px Arial' && a.font===(.05*H)+'px Arial' && a.args[1]===Math.round(.5*W)/2 && a.args[2]===Math.round(.1*H)/2 && Math.abs(b.args[2]-a.args[2]-L)<1e-6 && a.fill==='#0000ff' &&
                    __ioCalls.some(x=>x.method==='scale' && x.args[0]===2 && x.args[1]===2) && __ioCalls.some(x=>x.method==='rotate' && x.args[0]===Math.PI/2) &&
                    __ioCalls.findIndex(x=>x.method==='fillRect' && x.fill==='#ffffff') < __ioCalls.findIndex(x=>x.method==='fillText');
                })()
                """))
            #expect(try await Self.pixel(page, ".5*W+4", ".7*H+4") == Self.clear)
            #expect(try await page.bool("""
                (()=>{
                  const c=document.getElementById('image-occlusion-canvas'),W=c.width,H=c.height,probe=document.createElement('canvas').getContext('2d');
                  probe.font=(.1*H)+'px Arial';probe.textBaseline='top';const m=probe.measureText('M');
                  const box=__ioCalls.find(x=>x.method==='fillRect' && x.fill==='#ffffff');
                  const x=Math.round(.1*W),y=Math.round(.1*H),w=probe.measureText('Label').width+5,h=1.5*(m.actualBoundingBoxAscent+m.actualBoundingBoxDescent)+5,context=c.getContext('2d');
                  const pixel=(a,b)=>Array.from(context.getImageData(a,b,1,1).data).join(',');
                  return pixel(x+1,y+Math.floor(h)-1)==='255,255,255,255' && pixel(x+Math.ceil(w)+1,y+Math.floor(h)-1)==='0,0,0,0' && pixel(x+1,y+Math.ceil(h)+1)==='0,0,0,0' && box.args[0]===Math.round(.1*W) && box.args[1]===Math.round(.1*H) && Math.abs(box.args[2]-probe.measureText('Label').width-5)<1 && Math.abs(box.args[3]-1.5*(m.actualBoundingBoxAscent+m.actualBoundingBoxDescent)-5)<1;
                })()
                """))
            #expect(try await Self.pixel(page, "Math.round(.1*W)+1", "Math.round(.1*H)+1") == Self.white)
            // Invalid colours retain the previously assigned style; width zero
            // suppresses a stroke without changing the context's current width.
            #expect(try await page.bool("""
                (()=>{
                  const a=anki.imageOcclusion,c=document.createElement('canvas').getContext('2d');
                  c.lineWidth=7;c.font='17px serif';c.textBaseline='bottom';c.translate(2,3);
                  const shape=new a.Rectangle({width:.5,height:.5});
                  a.drawShape({context:c,size:{width:100,height:100},shape:shape,fill:'#00ff00',stroke:'#123456',strokeWidth:0});
                  a.drawShape({context:c,size:{width:100,height:100},shape:shape,fill:'not-a-color',stroke:'also-bad',strokeWidth:NaN});
                  return c.fillStyle==='#00ff00' && c.strokeStyle==='#123456' && c.lineWidth===7 && c.font==='17px serif' && c.textBaseline==='bottom' && c.getTransform().e===2 && c.getTransform().f===3;
                })()
                """))
            #expect(harness.diagnostics.isEmpty)
        }

        /// Local review artifacts only: native WebKit snapshot, no app launch,
        /// screen capture, baseline, or assertion substitute.
        static func snapshot(_ page: TestPage, _ name: String) async throws {
            #if os(macOS)
            guard page.harness.platform == .mac, let directory = ProcessInfo.processInfo.environment["MG_RUN_DIR"] else { return }
            let leaf = URL(fileURLWithPath: directory).appending(path: "io-visual", directoryHint: .isDirectory)
            try FileManager.default.createDirectory(at: leaf, withIntermediateDirectories: true)
            let image = try await page.webView.takeSnapshot(configuration: nil)
            let tiff = try #require(image.tiffRepresentation)
            let bitmap = try #require(NSBitmapImageRep(data: tiff))
            let png = try #require(bitmap.representation(using: .png, properties: [:]))
            try png.write(to: leaf.appending(path: "\(name).png"))
            #endif
        }
    }
}
