#!/usr/bin/env python3
"""Compare a baseline and candidate using deterministic harmless ZIP fixtures."""
import argparse, json, os, pathlib, random, shutil, statistics, subprocess, tempfile, time, zipfile
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('baseline')
parser.add_argument('candidate')
parser.add_argument('--output',default=str(pathlib.Path(__file__).resolve().parents[1]/'docs/benchmark-nested.jsonl'))
args=parser.parse_args()
BASE=str(pathlib.Path(args.baseline).resolve())
NEW=str(pathlib.Path(args.candidate).resolve())
OUTPUT=args.output
rng=random.Random(12345)

def archive(path, entries):
    with zipfile.ZipFile(path,'w',compression=zipfile.ZIP_DEFLATED) as z:
        for name, data in entries: z.writestr(name,data)

def run(binary, args):
    with tempfile.NamedTemporaryFile() as stat:
        start=time.monotonic()
        process=subprocess.Popen(['/usr/bin/time','-f','%M','-o',stat.name,binary,'--json',*map(str,args)],stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        staging=pathlib.Path(args[args.index('--output')+1] if '--output' in args else args[1])/'.staging'
        peak=0
        while True:
            try:
                stdout,stderr=process.communicate(timeout=0.005)
                break
            except subprocess.TimeoutExpired:
                size=0
                for directory,_,files in os.walk(staging):
                    for name in files:
                        try:size+=(pathlib.Path(directory)/name).stat().st_size
                        except FileNotFoundError:pass
                peak=max(peak,size)
        elapsed=time.monotonic()-start
        if process.returncode: raise RuntimeError(stderr.decode())
        report=json.loads(stdout)
        report['_sampled_peak_staging_bytes']=peak
        return elapsed,int(pathlib.Path(stat.name).read_text().strip()),report

def emit(f, value): f.write(json.dumps(value)+'\n'); f.flush()
with tempfile.TemporaryDirectory(prefix='bfsort-qualification-') as t,open(OUTPUT,'w') as out:
    root=pathlib.Path(t)
    emit(out,{'kind':'environment','seed':12345,'warmup_runs':1,'measured_runs':5,'workers':1,'cache':'warm OS cache; isolated repositories; no cache eviction','filesystem':subprocess.check_output(['stat','-f','-c','%T',t],text=True).strip(),'baseline':subprocess.check_output([BASE,'--version'],text=True).strip(),'candidate':subprocess.check_output([NEW,'--version'],text=True).strip()})
    for scenario, count, size in [('many-small',1000,1024),('few-large',4,8*1024*1024)]:
        source=root/(scenario+'.zip'); archive(source,[(f'{i}.bin',rng.randbytes(size)) for i in range(count)])
        for mode in ['fresh','resume']:
            results={}
            for label,binary in [('baseline',BASE),('candidate',NEW)]:
                vals=[]
                for i in range(6):
                    repo=root/f'{scenario}-{mode}-{label}-{i}'
                    args=['ingest',source,'--extract','--security-mode','standard','--workers','1','--output',repo,'--resume']
                    if mode=='resume':run(binary,args)
                    elapsed,rss,report=run(binary,args)
                    assert report['summary']['occurrences']==count
                    if i: vals.append(elapsed)
                    emit(out,{'kind':'run','case':scenario,'mode':mode,'implementation':label,'iteration':i,'warmup':i==0,'entries':count,'expanded_bytes':count*size,'wall_seconds':elapsed,'peak_rss_kib':rss,'sampled_peak_staging_bytes':report['_sampled_peak_staging_bytes']})
                    shutil.rmtree(repo)
                results[label]=statistics.median(vals)
            emit(out,{'kind':'comparison','case':scenario,'mode':mode,'baseline_median_seconds':results['baseline'],'candidate_median_seconds':results['candidate'],'ratio':results['candidate']/results['baseline'],'threshold':1.10})
    leaf=root/'leaf.zip';archive(leaf,[('a.pdf',b'%PDF-1.7\n'+rng.randbytes(1024*1024))])
    child=root/'child.zip';archive(child,[('leaf.zip',leaf.read_bytes()),('b.txt',b'hello')])
    source=root/'nested.zip';archive(source,[(f'{i}.zip',child.read_bytes()) for i in range(10)])
    for depth in [1,2]:
        config=root/f'depth{depth}.toml';config.write_text(f"input_mode = 'archives'\n[security]\nmode = 'standard'\n[archive]\nnested_extraction = true\nmax_depth = {depth}\nmax_total_entries = 1000\nmax_total_expanded_bytes = '100 MiB'\n")
        for mode in ['fresh','resume','partial-resume']:
            for i in range(6):
                repo=root/f'nested-{depth}-{mode}-{i}';args=['ingest',source,'-c',config,'--workers','1','--output',repo,'--resume']
                if mode!='fresh':
                    run(NEW,args)
                    if mode=='partial-resume':
                        # Remove one receipt only from this disposable synthetic repository.
                        receipts=list((repo/'operations'/'extraction').glob('*.json'))
                        target=next(p for p in receipts if json.loads(p.read_text())['archive_hash']==__import__('hashlib').sha256(child.read_bytes()).hexdigest())
                        target.unlink()
                elapsed,rss,report=run(NEW,args)
                metrics=report['nested']['metrics']
                assert metrics['extracted_archives']==(depth+1 if mode=='fresh' else 0 if mode=='resume' else 1),metrics
                emit(out,{'kind':'nested','depth':depth,'mode':mode,'iteration':i,'warmup':i==0,'wall_seconds':elapsed,'peak_rss_kib':rss,'sampled_peak_staging_bytes':report['_sampled_peak_staging_bytes'],'metrics':metrics,'occurrences':report['summary']['occurrences']})
    analysis_config=root/'analysis.toml'
    analysis_config.write_text("input_mode = 'archives'\n[archive]\nnested_extraction = true\nmax_depth = 1\nmax_total_entries = 1000\nmax_total_expanded_bytes = '100 MiB'\n")
    for i in range(6):
        existing=root/f'existing-{i}'
        run(NEW,['ingest',source,'--extract','--security-mode','standard','--output',existing])
        elapsed,rss,analysis=run(NEW,['analyze',existing,'-c',analysis_config,'--resume'])
        _,_,stats=run(NEW,['stats',existing])
        assert stats['occurrences']==40
        emit(out,{'kind':'existing-analysis','iteration':i,'warmup':i==0,'wall_seconds':elapsed,'peak_rss_kib':rss,'sampled_peak_staging_bytes':analysis['_sampled_peak_staging_bytes'],'metrics':analysis['metrics'],'final_occurrences':stats['occurrences']})
    elapsed,rss,report=run(NEW,['reclassify',repo,'--resume'])
    emit(out,{'kind':'reclassify','wall_seconds':elapsed,'peak_rss_kib':rss,'report':report,'expected_decompressions':0})
print(OUTPUT)
