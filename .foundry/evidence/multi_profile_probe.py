import json, os, pathlib, shutil, subprocess, sys, tempfile
repo = pathlib.Path(sys.argv[1])
with tempfile.TemporaryDirectory(prefix='cmx-multi-proof-') as tmp:
    base = pathlib.Path(tmp)
    atlas = base / 'atlas'
    shutil.copytree(repo / 'reference-atlas', atlas)
    profile = (atlas / 'profiles/rust-shipping.toml').read_text()
    (atlas / 'profiles/rust-skill.toml').write_text(profile.replace('id = "rust-shipping"', 'id = "rust-skill"').replace('name = "AGENTS"', 'name = "rust-skill"').replace('surface = "agent"', 'surface = "skill"'))
    project = base / 'project'
    shutil.copytree(atlas / 'cases/compliant', project)
    home = base / 'home'
    home.mkdir()
    env = dict(os.environ, HOME=str(home), XDG_CONFIG_HOME=str(home / '.config'), XDG_CACHE_HOME=str(home / '.cache'))
    def run(name, *args):
        cmd = [str(repo / 'target/debug' / name), *map(str,args)]
        result = subprocess.run(cmd, cwd=project, env=env, text=True, capture_output=True)
        print('$', ' '.join(cmd), '\nexit:', result.returncode, '\nstdout:\n', result.stdout, '\nstderr:\n', result.stderr, flush=True)
        if result.returncode: raise RuntimeError(f'{name} failed')
        return result
    run('cmf', '--root', atlas, 'install', 'rust-shipping', '--local', '--platform', 'codex', '--apply')
    run('cmf', '--root', atlas, 'install', 'rust-skill', '--local', '--platform', 'codex', '--apply')
    manifest = json.loads((project / '.context-mixer/cmf-manifest.json').read_text())
    print('manifest:', json.dumps(manifest, indent=2), flush=True)
    records = manifest.get('artifacts', [])
    assert len(records) == 2, f'expected two retained compilation records; got {len(records)}'
    assert {r['artifact']['surface'] for r in records} == {'agent','skill'}
    result = run('cmv', 'check', '--json', '--atlas', atlas)
    report = json.loads(result.stdout)
    assert len(report['artifacts']) == 2, 'check must report both artifacts'
