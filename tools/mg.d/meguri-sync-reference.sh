# Quarantined current-release client/reference work; no production imports.
mg_job sync-server-reference host mg_sync_server_reference \
  "Pinned current-release reference harness, semantic validation and synthetic probes"

mg_sync_reference_digest() {
  (
    set -o pipefail
    cd "$MG_ROOT" || exit 1
    rg --files oracle/meguri-sync-server docker/meguri-sync-reference.Dockerfile \
      docker/meguri-sync-reference.Dockerfile.dockerignore docker/meguri-sync-reference.compose.yml \
      tools/mg tools/mg.d/meguri-sync.sh tools/mg.d/meguri-sync-reference.sh conformance/schema/meguri-sync-server.schema.json \
      conformance/catalog/meguri-sync-server.json conformance/fixtures/meguri-sync-server/original.json \
      conformance/recordings/scenarios/meguri-sync-server/desktop-selection.json \
      conformance/fixtures/meguri-sync-server/edges.json conformance/recordings/scenarios/meguri-sync-server/desktop-edge-selection.json \
      conformance/recordings/scenarios/meguri-sync-server/candidate-normal-selection.json \
      conformance/recordings/scenarios/meguri-sync-server/candidate-media-selection.json \
      conformance/recordings/scenarios/meguri-sync-server/desktop-media-selection.json \
      conformance/fixtures/meguri-sync-server/media.json \
      conformance/fixtures/meguri-sync-server/recovery.json \
      conformance/fixtures/meguri-sync-server/recovery-dirty.json \
      conformance/recordings/scenarios/meguri-sync-server/desktop-recovery-dirty-selection.json \
      conformance/recordings/scenarios/meguri-sync-server/desktop-recovery-selection.json \
      conformance/recordings/scenarios/meguri-sync-server/candidate-full-selection.json \
      conformance/recordings/scenarios/meguri-sync-server/candidate-complete-selection.json \
      conformance/recordings/scenarios/meguri-sync-server/candidate-final-additions.json \
      conformance/recordings/scenarios/meguri-sync-server/candidate-retained-final-selection.json \
      conformance/recordings/scenarios/meguri-sync-server/candidate-mobile-final-selection.json \
      conformance/recordings/scenarios/meguri-sync-server/final-fixture-recipes.json |
      LC_ALL=C sort | while IFS= read -r file; do shasum -a 256 "$file" || exit 1; done | shasum -a 256 | cut -d ' ' -f1
  )
}

# This record is outside the only child-writable mount. Both the job's exit
# hook and MG's cancellation parent use it; refusal never permits a label sweep.
mg_sync_reference__cancel_cleanup() {
  local ownership name owner cleanup rc=0
  ownership="$(python3 - "$MG_RUN_DIR/reference-control" "$MG_RUN_ID" <<'PY'
import json, os, pathlib, re, stat, sys
def require(value):
    if not value:
        raise SystemExit('invalid reference ownership')
directory = pathlib.Path(sys.argv[1])
s = directory.lstat()
require(stat.S_ISDIR(s.st_mode) and s.st_uid == os.getuid() and stat.S_IMODE(s.st_mode) == 0o700)
path = directory / 'owner.json'
s = path.lstat()
require(stat.S_ISREG(s.st_mode) and s.st_uid == os.getuid() and s.st_nlink == 1 and stat.S_IMODE(s.st_mode) == 0o600)
# A parent record is small; bound both the admitted size and the actual read
# before decoding or parsing, including growth after the metadata check.
maximum_bytes = 4096
require(0 < s.st_size <= maximum_bytes)
with os.fdopen(os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK), 'rb') as stream:
    opened = os.fstat(stream.fileno())
    require((opened.st_dev, opened.st_ino) == (s.st_dev, s.st_ino))
    require(stat.S_ISREG(opened.st_mode) and opened.st_uid == os.getuid() and opened.st_nlink == 1
            and stat.S_IMODE(opened.st_mode) == 0o600 and 0 < opened.st_size <= maximum_bytes)
    contents = stream.read(maximum_bytes + 1)
require(len(contents) == opened.st_size and len(contents) <= maximum_bytes)
def unique(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result)
        result[key] = value
    return result
record = json.loads(contents.decode('utf-8'), object_pairs_hook=unique)
require(set(record) == {'run', 'name', 'nonce'} and record['run'] == sys.argv[2])
require(re.fullmatch(r'[A-Za-z0-9_-]+', record['run']))
require(isinstance(record['nonce'], str) and re.fullmatch(r'[0-9a-f]{32}', record['nonce']))
require(record['name'] == 'meguri-reference-' + record['run'] + '-' + record['nonce'])
print(record['name'])
print(record['nonce'])
PY
  )" || {
    mg_failure reference-cleanup "Missing or invalid parent ownership; no broad container cleanup is permitted"
    mg_status fail
    return 1
  }
  name="${ownership%%$'\n'*}" owner="${ownership#*$'\n'}"
  mg_sync_server__remove_container "$name" "$owner" || rc=$?
  if ! cleanup="$(jq -nc --argjson rc "$rc" \
      '{checked:true,exit_code:$rc,absence_verified:($rc==0),late_create_excluded:false}')" ||
      ! mg_detail reference_cleanup "$cleanup"; then
    # EXIT runners discard hook returns. Persist the failure here so an
    # earlier pass or a later successful cleanup cannot hide lost evidence.
    mg_failure reference-cleanup-publication "Could not publish owned reference cleanup evidence; no compatibility credit"
    mg_status fail
    [ "$rc" -ne 0 ] || rc=1
  fi
  return "$rc"
}

# Read Docker stdout as bytes before any shell capture, retaining its whitespace
# in the raw hash. Publish only finite identity/config fields and hash metadata.
# In particular, neither arbitrary environment values nor State.Error escape.
mg_sync_reference__inspect() {
  local measured rc=0 stage pipeline_status
  # Parent-only diagnostics: hash each complete property, including all nested
  # values, but emit only static top-level Docker schema names. Preserve these
  # raw diagnostics separately from the one named effective comparison below.
  measured="$(
    # Check both processes explicitly, including valid output followed by a
    # failing Docker exit. Only validated projection/fixed metadata is captured.
    set +e
    docker inspect "$1" 2>/dev/null | python3 -c '
import hashlib, json, re, sys
def require(condition):
    if not condition:
        raise ValueError()
def unique(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result)
        result[key] = value
    return result
def invalid_number(value):
    raise ValueError()
def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"),
                      ensure_ascii=True, allow_nan=False).encode("ascii")
stage = "read"
try:
    # Bound decoding as well as the final diagnostic representation. No parse
    # error or exception text (which could contain raw values) is published.
    raw = sys.stdin.buffer.read(8 * 1024 * 1024 + 1)
    stage = "bounds"
    require(0 < len(raw) <= 8 * 1024 * 1024)
    # Docker diagnostics support integer numbers only. Refuse decimal/exponent
    # tokens before conversion can round them or underflow them to zero.
    stage = "json"
    require(b"\x00" not in raw)
    records = json.loads(raw.decode("utf-8"), object_pairs_hook=unique,
                         parse_float=invalid_number, parse_constant=invalid_number)
    stage = "shape"
    require(type(records) is list and len(records) == 1 and type(records[0]) is dict)
    record = records[0]
    config_names = set("""Hostname Domainname User AttachStdin AttachStdout AttachStderr
        ExposedPorts Tty OpenStdin StdinOnce Env Cmd Healthcheck ArgsEscaped Image
        Volumes WorkingDir Entrypoint NetworkDisabled MacAddress OnBuild Labels
        StopSignal StopTimeout Shell""".split())
    host_names = set("""Binds ContainerIDFile LogConfig NetworkMode PortBindings
        RestartPolicy AutoRemove VolumeDriver VolumesFrom ConsoleSize CapAdd CapDrop
        CgroupnsMode Dns DnsOptions DnsSearch ExtraHosts GroupAdd IpcMode Cgroup Links
        OomScoreAdj PidMode Privileged PublishAllPorts ReadonlyRootfs SecurityOpt
        StorageOpt Tmpfs UTSMode UsernsMode ShmSize Sysctls Runtime Isolation
        CpuShares Memory NanoCpus CgroupParent BlkioWeight BlkioWeightDevice
        BlkioDeviceReadBps BlkioDeviceWriteBps BlkioDeviceReadIOps BlkioDeviceWriteIOps
        CpuPeriod CpuQuota CpuRealtimePeriod CpuRealtimeRuntime CpusetCpus CpusetMems
        Devices DeviceCgroupRules DeviceRequests KernelMemory KernelMemoryTCP
        MemoryReservation MemorySwap MemorySwappiness OomKillDisable PidsLimit
        Ulimits CpuCount CpuPercent IOMaximumIOps IOMaximumBandwidth Mounts MaskedPaths
        ReadonlyPaths Init Annotations""".split())
    # Empty ExposedPorts/Volumes may be omitted. Hash actual keys only: absent
    # and present-null must remain different inventories and raw configurations.
    required_config = set("""Entrypoint Cmd User Image WorkingDir Labels Env""".split())
    required_host = set("""NetworkMode PortBindings PublishAllPorts Init ReadonlyRootfs
        CapDrop CapAdd SecurityOpt Memory OomKillDisable PidsLimit AutoRemove RestartPolicy
        Privileged Devices DeviceRequests VolumesFrom Links ExtraHosts PidMode
        IpcMode UTSMode UsernsMode Binds Tmpfs Mounts""".split())
    inventory = {}
    for section, allowed, required, limit in (
            ("Config", config_names, required_config, 32),
            ("HostConfig", host_names, required_host, 96)):
        values = record.get(section)
        stage = "shape"
        require(type(values) is dict)
        stage = "bounds"
        require(0 < len(values) <= limit)
        stage = "schema"
        require(values.keys() <= allowed)
        require(all(re.fullmatch(r"[A-Z][A-Za-z0-9]{0,39}", name) for name in values))
        stage = "required"
        require(required <= values.keys())
        stage = "canonical"
        inventory[section] = {name: hashlib.sha256(canonical(value)).hexdigest()
                              for name, value in values.items()}
    stage = "required"
    require("Path" in record and "Args" in record)
    stage = "shape"
    require(type(record.get("Path")) is str and type(record.get("Args")) is list
            and all(type(arg) is str for arg in record["Args"]))
    stage = "canonical"
    for name in ("Path", "Args"):
        inventory[name] = hashlib.sha256(canonical(record[name])).hexdigest()
    stage = "bounds"
    require(len(inventory) == 4 and sum(len(inventory[s]) for s in ("Config", "HostConfig")) + 2 <= 130)
    stage = "canonical"
    encoded = canonical(inventory)
    stage = "bounds"
    require(len(encoded) <= 16384)
    stage = "canonical"
    # Full configuration and individual properties share an exact integer,
    # sorted-key ASCII encoding, without whitespace or a trailing newline.
    configuration = {name: record[name] for name in ("Config", "HostConfig", "Path", "Args")}
    configuration_hash = hashlib.sha256(canonical(configuration)).hexdigest()
    # Docker documents oom-kill-disable as discarded on cgroup v2; Moby uses
    # false by default and nil when unsupported. Only these two present values
    # represent the required enabled OOM killer here. This is no observation of
    # the installed daemon/cgroup. Do not mutate the original record or hashes.
    oom_killer_enabled_hash = None
    host = record["HostConfig"]
    if host["OomKillDisable"] is False or host["OomKillDisable"] is None:
        effective = dict(configuration, HostConfig=dict(host, OomKillDisable=False))
        oom_killer_enabled_hash = hashlib.sha256(
            b"meguri-oom-killer-enabled-configuration-v1\x00" + canonical(effective)).hexdigest()
    stage = "shape"
    config, host = record["Config"], record["HostConfig"]
    labels, env = config["Labels"], config["Env"]
    state, network, mounts = record.get("State"), record.get("NetworkSettings"), record.get("Mounts")
    require(type(labels) is dict and type(env) is list and all(type(item) is str for item in env))
    require(type(state) is dict and type(network) is dict and type(network.get("Networks")) is dict)
    require(type(mounts) is list and all(type(item) is dict for item in mounts))
    def pick(value, names):
        return {name: value.get(name) for name in names.split()}
    def default(value, fallback):
        return fallback if value is None or value is False else value
    projected_host = pick(host, """NetworkMode PortBindings PublishAllPorts Init ReadonlyRootfs
        CapDrop CapAdd SecurityOpt Memory PidsLimit AutoRemove RestartPolicy Privileged
        Devices DeviceRequests VolumesFrom Links ExtraHosts PidMode IpcMode UTSMode UsernsMode Binds Tmpfs""")
    host_mounts = host["Mounts"]
    require(host_mounts is None or type(host_mounts) is list)
    projected_host["Mounts"] = []
    for mount in host_mounts or []:
        require(type(mount) is dict)
        bind = mount.get("BindOptions")
        require(bind is None or type(bind) is dict)
        bind = bind if bind is not None else {}
        projected_host["Mounts"].append(dict(pick(mount, "Type Source Target"),
            ReadOnly=default(mount.get("ReadOnly"), False),
            propagation=default(bind.get("Propagation"), ""),
            extra_options=(mount.get("VolumeOptions") is not None or mount.get("TmpfsOptions") is not None
                           or (bind.get("NonRecursive") is not None and bind.get("NonRecursive") is not False))))
    container = pick(record, "Id Image Name Created RestartCount Path Args")
    container.update(
        Config=dict(pick(config, "Entrypoint Cmd User Image WorkingDir ExposedPorts Volumes"),
            Labels={"run": labels.get("meguri.mg.run-id"), "nonce": labels.get("meguri.sync.owner")},
            run_environment=[item for item in env if item.startswith("MG_RUN_ID=")]),
        HostConfig=projected_host,
        Mounts=[pick(mount, "Type Source Destination RW") for mount in mounts],
        NetworkSettings={"Ports": network.get("Ports"), "networks": sorted(network["Networks"])},
        State=dict(pick(state, "Status Running Paused Restarting Dead OOMKilled ExitCode StartedAt FinishedAt"),
                   error_present=(state.get("Error") != "")))
    stage = "canonical"
    encoded = canonical(dict(inspection_sha256=hashlib.sha256(raw).hexdigest(),
        configuration_sha256=configuration_hash, property_fingerprints=inventory,
        oom_killer_enabled_configuration_sha256=oom_killer_enabled_hash, container=container))
except (ValueError, TypeError, KeyError, OverflowError, RecursionError, OSError):
    # Only author-owned finite stages escape, never input names or exceptions.
    print(stage)
    sys.exit(1)
sys.stdout.buffer.write(encoded)
    ' 2>/dev/null
    pipeline_status=("${PIPESTATUS[@]}")
    [ "${pipeline_status[0]}" -eq 0 ] || exit 2
    [ "${pipeline_status[1]}" -eq 0 ] || exit 1
  )" || rc=$?
  if [ "$rc" -ne 0 ]; then
    stage=read
    if [ "$rc" -eq 1 ]; then
      case "$measured" in read|bounds|json|shape|schema|required|canonical) stage="$measured" ;; esac
    fi
    printf 'reference inspection refused: %s\n' "$stage" >&2
    return 1
  fi
  printf '%s\n' "$measured"
}

mg_sync_reference__check() {
  local measured="$1" plan="$2" before="$3" phase="$4" ceiling
  ceiling="$(date -u +%Y-%m-%dT%H:%M:%S)" || return 1
  jq -e --argjson p "$plan" --argjson before "$before" --arg phase "$phase" --arg ceiling "$ceiling" '
    def timestamp: type=="string" and test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(\\.[0-9]{1,9})?Z$");
    def instant: rtrimstr("Z") | split(".") | .[0]+"."+(((.[1] // "")+"000000000")[0:9]);
    def digest: type=="string" and test("^[0-9a-f]{64}$");
    # Exact canonical false/null hashes; missing, true, integers, strings and
    # containers cannot satisfy this property-specific admission in either phase.
    def oom_killer_enabled:
      (.property_fingerprints.HostConfig.OomKillDisable as $oom |
        $oom=="fcbcf165908dd18a9e49f7ff27810176db8e9f63b4352213741664245224f8aa" or
        $oom=="74234e98afe7498fb5daf1f36ac2d78acc339464f950703b8c019892f982b90b") and
      (.oom_killer_enabled_configuration_sha256 | digest);
    def properties($limit): type=="object" and length>0 and length<=$limit and
      all(to_entries[]; (.key | test("^[A-Z][A-Za-z0-9]{0,39}$")) and (.value | digest));
    def inventory: type=="object" and keys==["Args","Config","HostConfig","Path"] and
      (.Config | properties(32)) and (.HostConfig | properties(96)) and
      (.Path | digest) and (.Args | digest) and (tojson | length<=16384);
    # Same finite Docker schema as the producer; caller-shaped property names
    # are never admitted by the predicate, even when their hashes look valid.
    def config_names: "Hostname Domainname User AttachStdin AttachStdout AttachStderr ExposedPorts Tty OpenStdin StdinOnce Env Cmd Healthcheck ArgsEscaped Image Volumes WorkingDir Entrypoint NetworkDisabled MacAddress OnBuild Labels StopSignal StopTimeout Shell" | split(" ");
    def host_names: "Binds ContainerIDFile LogConfig NetworkMode PortBindings RestartPolicy AutoRemove VolumeDriver VolumesFrom ConsoleSize CapAdd CapDrop CgroupnsMode Dns DnsOptions DnsSearch ExtraHosts GroupAdd IpcMode Cgroup Links OomScoreAdj PidMode Privileged PublishAllPorts ReadonlyRootfs SecurityOpt StorageOpt Tmpfs UTSMode UsernsMode ShmSize Sysctls Runtime Isolation CpuShares Memory NanoCpus CgroupParent BlkioWeight BlkioWeightDevice BlkioDeviceReadBps BlkioDeviceWriteBps BlkioDeviceReadIOps BlkioDeviceWriteIOps CpuPeriod CpuQuota CpuRealtimePeriod CpuRealtimeRuntime CpusetCpus CpusetMems Devices DeviceCgroupRules DeviceRequests KernelMemory KernelMemoryTCP MemoryReservation MemorySwap MemorySwappiness OomKillDisable PidsLimit Ulimits CpuCount CpuPercent IOMaximumIOps IOMaximumBandwidth Mounts MaskedPaths ReadonlyPaths Init Annotations" | split(" ");
    .container as $c | $c.HostConfig as $h |
    (.inspection_sha256 | digest) and (.configuration_sha256 | digest) and
    oom_killer_enabled and
    (.property_fingerprints | inventory) and
    ((.property_fingerprints.Config | keys) - config_names == []) and
    ((.property_fingerprints.HostConfig | keys) - host_names == []) and
    ((.property_fingerprints.Config | keys) | contains(["Cmd","Entrypoint","Env","Image","Labels","User","WorkingDir"])) and
    ((.property_fingerprints.HostConfig | keys) | contains($h | keys)) and
    ($c.Id==$p.id and ($c.Id | test("^[0-9a-f]{64}$"))) and
    $c.Image==$p.image and $c.Config.Image==$p.image and $c.Name==("/"+$p.name) and
    ($c.Created | timestamp) and $c.Created[0:19]>=$p.creation_floor and $c.Created[0:19]<=$ceiling and
    $c.Config.Entrypoint==["python","/opt/reference/driver.py"] and $c.Config.Cmd==[$p.mode] and
    $c.Path=="python" and $c.Args==["/opt/reference/driver.py",$p.mode] and
    $c.Config.User==$p.user and $c.Config.WorkingDir=="/scratch" and
    $c.Config.Labels=={run:$p.run,nonce:$p.nonce} and $c.Config.run_environment==["MG_RUN_ID="+$p.run] and
    (($c.Config.ExposedPorts // {})=={}) and (($c.Config.Volumes // {})=={}) and
    $h.NetworkMode=="none" and (($h.PortBindings // {})=={}) and $h.PublishAllPorts==false and
    $c.NetworkSettings.Ports=={} and ($c.NetworkSettings.networks==[] or $c.NetworkSettings.networks==["none"]) and
    $h.Init==true and $h.ReadonlyRootfs==true and $h.CapDrop==["ALL"] and (($h.CapAdd // [])==[]) and
    $h.SecurityOpt==["no-new-privileges"] and $h.Memory==2147483648 and $h.PidsLimit==128 and
    $h.AutoRemove==false and $h.RestartPolicy=={Name:"no",MaximumRetryCount:0} and
    $h.Privileged==false and (($h.Devices // [])==[]) and (($h.DeviceRequests // [])==[]) and
    (($h.VolumesFrom // [])==[]) and (($h.Links // [])==[]) and (($h.ExtraHosts // [])==[]) and
    $h.PidMode=="" and $h.IpcMode=="private" and $h.UTSMode=="" and $h.UsernsMode=="" and
    (($h.Binds // [])==[]) and
    ($h.Mounts | length==1 and .[0].Type=="bind" and .[0].Source==$p.output and .[0].Target=="/out" and
      .[0].ReadOnly==false and (.[0].propagation=="" or .[0].propagation=="rprivate") and .[0].extra_options==false) and
    $h.Tmpfs=={"/scratch":"exec,mode=1777,size=1g","/tmp":"mode=1777,size=64m"} and
    ([$c.Mounts[] | select(.Type!="tmpfs")]==[{Type:"bind",Source:$p.output,Destination:"/out",RW:true}]) and
    ([$c.Mounts[] | select(.Type=="tmpfs")] as $t |
      ($t | length)<=2 and ([$t[].Destination] | unique | length)==($t | length) and
      all($t[]; (.Destination=="/scratch" or .Destination=="/tmp") and .RW==true and (.Source=="" or .Source==null))) and
    $c.RestartCount==0 and $c.State.Paused==false and $c.State.Restarting==false and $c.State.Dead==false and
    $c.State.error_present==false and
    (if $phase=="before" then
       $c.State.Status=="created" and $c.State.Running==false and $c.State.OOMKilled==false and $c.State.ExitCode==0 and
       $c.State.StartedAt=="0001-01-01T00:00:00Z" and $c.State.FinishedAt=="0001-01-01T00:00:00Z"
     else
       ($before | oom_killer_enabled) and
       .oom_killer_enabled_configuration_sha256==$before.oom_killer_enabled_configuration_sha256 and
       (if .property_fingerprints.HostConfig.OomKillDisable==$before.property_fingerprints.HostConfig.OomKillDisable then
          .configuration_sha256==$before.configuration_sha256 and
          .property_fingerprints==$before.property_fingerprints
        else
          # A false/null transition is effective equality, never raw equality.
          .configuration_sha256!=$before.configuration_sha256 and
          (.property_fingerprints | del(.HostConfig.OomKillDisable))==
            ($before.property_fingerprints | del(.HostConfig.OomKillDisable))
        end) and
       ($c | del(.State,.NetworkSettings))==($before.container | del(.State,.NetworkSettings)) and
       $c.State.Status=="exited" and $c.State.Running==false and
       ($c.State.OOMKilled | type=="boolean") and
       ($c.State.ExitCode | type=="number" and floor==. and .>=0 and .<=255) and
       ($c.State.StartedAt | timestamp) and ($c.State.FinishedAt | timestamp) and
       ($c.State.StartedAt | instant)>=($c.Created | instant) and
       ($c.State.FinishedAt | instant)>=($c.State.StartedAt | instant) and $c.State.FinishedAt[0:19]<=$ceiling
     end)
  ' <<<"$measured" >/dev/null 2>&1
}

mg_sync_reference__retained() {
  local mode="$1" output="$2" image owner name control="$MG_RUN_DIR/reference-control"
  local plan cid before=null after=null rc=0 cleanup_rc=0 attach_rc=null actual_exit=null valid=false invocation_sha
  local before_valid=false
  image="$(mg_sync_server__image sync-reference -f "$MG_ROOT/docker/meguri-sync-reference.compose.yml")" || return 1
  [[ "$image" =~ ^sha256:[0-9a-f]{64}$ ]] || return 1
  owner="$(python3 -c 'import secrets; print(secrets.token_hex(16))')" || return 1
  [[ "$MG_RUN_ID" =~ ^[A-Za-z0-9_-]+$ ]] && [[ "$owner" =~ ^[0-9a-f]{32}$ ]] || return 1
  name="meguri-reference-$MG_RUN_ID-$owner"
  mkdir -m 700 "$control" "$output" || return 1
  # Publish atomically before any uncertain daemon operation. Neither this
  # record nor the original invocation/pre-inspection is accessible via /out.
  (umask 077; jq -nc --arg run "$MG_RUN_ID" --arg name "$name" --arg nonce "$owner" \
    '{run:$run,name:$name,nonce:$nonce}' >"$control/owner.pending" && mv "$control/owner.pending" "$control/owner.json") || return 1
  mg_on_exit mg_sync_reference__cancel_cleanup
  cp "$MG_RUN_DIR/invocation.json" "$control/invocation.json" && cp "$control/invocation.json" "$output/invocation.json" || return 1
  invocation_sha="$(shasum -a 256 "$control/invocation.json" | cut -d ' ' -f1)" || return 1
  [[ "$invocation_sha" =~ ^[0-9a-f]{64}$ ]] || return 1
  plan="$(jq -nc --arg image "$image" --arg name "$name" --arg nonce "$owner" --arg run "$MG_RUN_ID" \
    --arg mode "$mode" --arg output "$output" --arg user "$(id -u):$(id -g)" \
    --arg floor "$(date -u +%Y-%m-%dT%H:%M:%S)" \
    '{image:$image,name:$name,nonce:$nonce,run:$run,mode:$mode,output:$output,user:$user,creation_floor:$floor}')" || return 1
  cid="$(docker create --pull never --name "$name" --label "meguri.mg.run-id=$MG_RUN_ID" \
    --label "meguri.sync.owner=$owner" --user "$(id -u):$(id -g)" --env "MG_RUN_ID=$MG_RUN_ID" \
    --network none --init --read-only --cap-drop ALL --security-opt no-new-privileges --memory 2g --pids-limit 128 \
    --tmpfs /scratch:exec,mode=1777,size=1g --tmpfs /tmp:mode=1777,size=64m \
    --mount "type=bind,src=$output,dst=/out" "$image" "$mode")" || rc=$?
  if [ "$rc" -eq 0 ] && [[ "$cid" =~ ^[0-9a-f]{64}$ ]]; then
    plan="$(jq -c --arg id "$cid" '. + {id:$id}' <<<"$plan")" || rc=1
    before="$(mg_sync_reference__inspect "$cid")" || { before=null; rc=1; }
    if [ "$rc" -eq 0 ] && mg_sync_reference__check "$before" "$plan" null before; then
      before_valid=true
      # One start only, and always inspect after attach, even on process failure.
      attach_rc=0
      docker start --attach "$cid" || attach_rc=$?
      rc="$attach_rc"
      after="$(mg_sync_reference__inspect "$cid")" || { after=null; [ "$rc" -ne 0 ] || rc=1; }
      # This scalar is published separately even when the full postcheck fails.
      # Never copy a rejected string/object (or out-of-range number) into it.
      actual_exit="$(jq -c '.container.State.ExitCode | if type=="number" then
        if floor==. and .>=0 and .<=255 then . else null end else null end' <<<"$after")" || actual_exit=null
      if mg_sync_reference__check "$after" "$plan" "$before" after; then
        valid=true
        if [ "$actual_exit" -ne 0 ] || [ "$(jq -r .container.State.OOMKilled <<<"$after")" != false ]; then
          [ "$rc" -ne 0 ] || rc=1
        fi
      else [ "$rc" -ne 0 ] || rc=1; fi
    else rc=1; fi
  else [ "$rc" -ne 0 ] || rc=1; fi
  mg_sync_reference__cancel_cleanup || cleanup_rc=$?
  [ "$rc" -ne 0 ] || rc="$cleanup_rc"
  # Child invocation mutation cannot replace the trusted original.
  if [ -L "$output/invocation.json" ] ||
      [ -z "$(find "$output/invocation.json" -prune -type f -links 1 -print)" ] ||
      ! cmp -s "$control/invocation.json" "$output/invocation.json"; then
    [ "$rc" -ne 0 ] || rc=1
  fi
  # Producer diagnostics are already bounded and safe. Only a full successful
  # check permits publication of a container projection, independently for
  # each phase. Keep the full measurements in parent memory for comparison.
  mg_detail reference_execution "$(jq -nc --arg image "$image" --argjson plan "$plan" --arg invocation "$invocation_sha" \
    --argjson before "$before" --argjson after "$after" --argjson attach "$attach_rc" --argjson exit "$actual_exit" \
    --argjson before_valid "$before_valid" --argjson cleanup "$cleanup_rc" --argjson valid "$valid" --argjson rc "$rc" \
    'def published($checked):
       if .==null or $checked then . else
         {inspection_sha256,configuration_sha256,property_fingerprints,
          oom_killer_enabled_configuration_sha256,validation_failure:"check"}
       end;
    {image_id:$image,container_id:($plan.id // null),invocation_sha256:$invocation,
      before:($before | published($before_valid)),after:($after | published($valid)),
      measurements_valid:$valid,attach_exit:$attach,actual_exit:$exit,
      cleanup_exit:$cleanup,absence_verified:($cleanup==0),execution_exit:$rc}')" || { [ "$rc" -ne 0 ] || rc=1; }
  return "$rc"
}

mg_sync_server_reference() {
  local mode="${1:-selftest}" reference_dir="$MG_ROOT/oracle/meguri-sync-server"
  [ "$#" -eq 0 ] || shift
  case "$mode" in
    deps)
      if [ "$#" -ne 0 ]; then mg_failure usage "deps takes no arguments"; mg_status error; return; fi
      local audit_status=0
      mg_compose -f "$MG_ROOT/docker/meguri-sync-reference.compose.yml" run --rm -T --no-deps \
        --label "meguri.mg.run-id=$MG_RUN_ID" -u "$(id -u):$(id -g)" \
        -v "$reference_dir:/lock:ro" -v "$MG_RUN_DIR:/out" \
        sync-reference-resolver python /lock/audit_deps.py || audit_status=$?
      if [ -f "$MG_RUN_DIR/summary.json" ]; then
        mg_count total "$(jq -r .total "$MG_RUN_DIR/summary.json")"
        mg_count passed "$(jq -r .passed "$MG_RUN_DIR/summary.json")"
        mg_count failed "$(jq -r .failed "$MG_RUN_DIR/summary.json")"
      fi
      mg_artifact report "$MG_RUN_DIR/dependencies.json" "Test-only exact dependency graph review"
      if [ "$audit_status" -ne 0 ]; then mg_failure dependency-audit "test dependency metadata/hash/advisory audit failed"; mg_status fail; else mg_status pass; fi
      return ;;
    lock)
      if [ "$#" -ne 0 ]; then
        mg_failure usage "lock takes no arguments"; mg_status error; return
      fi
      if ! mg_compose -f "$MG_ROOT/docker/meguri-sync-reference.compose.yml" run --rm -T --no-deps \
        --label "meguri.mg.run-id=$MG_RUN_ID" -v "$reference_dir:/lock" -v "$MG_RUN_DIR:/out" -w /lock \
        sync-reference-resolver sh -c 'pip install --quiet --disable-pip-version-check --root-user-action=ignore --require-hashes --no-deps --only-binary=:all: --report /out/resolver-install.json -r resolver-requirements.lock && uv pip compile --quiet --generate-hashes --python-version 3.12 --python-platform aarch64-manylinux_2_35 --custom-compile-command "tools/mg sync-server-reference lock" requirements.in -o requirements.lock'; then
        mg_failure dependency-lock "current reference dependency resolution failed"; mg_status fail; return
      fi
      mg_artifact lock "$reference_dir/requirements.lock"
      mg_artifact report "$MG_RUN_DIR/resolver-install.json" "Hash-pinned lock resolver installation"
      mg_status pass; return ;;
    selftest|identity|recovery-discover|recovery|recovery-dirty|smoke|edges|normal|normal-sdk|normal-matrix|media|catalog|validate|seal) ;;
    *) mg_failure usage "expected lock, deps, selftest, identity, recovery-discover, recovery, recovery-dirty, smoke, edges, normal, normal-sdk, normal-matrix, media, catalog, validate or seal"; mg_status error; return ;;
  esac
  local source_sha lock_sha source_commit author_session source_author_sessions argument_json
  local mount_args=() runtime_args=("$mode") input_path
  argument_json="$(printf '%s\n' "$mode" ${1+"$@"} | jq -Rs 'split("\n")[:-1]')"
  case "$mode" in
    validate|seal)
      if [ "$#" -ne 1 ]; then mg_failure usage "validate/seal requires a retained build/ evidence directory"; mg_status error; return; fi
      case "$1" in build/*) ;; *) mg_failure input-path "input must be under build/"; mg_status error; return ;; esac
      input_path="$(cd "$MG_ROOT/$1" && pwd -P)" || return 1
      case "$input_path" in "$MG_ROOT/build/"*) ;; *) mg_failure input-path "input escapes build/"; mg_status error; return ;; esac
      mount_args=(-v "$input_path:/in:ro")
      runtime_args=("$mode" --input /in) ;;
    smoke)
      if [ "$#" -gt 1 ]; then mg_failure usage "smoke accepts one optional build/ request JSON"; mg_status error; return; fi
      if [ "$#" -eq 1 ]; then
        case "$1" in build/*) ;; *) mg_failure input-path "request must be under build/"; mg_status error; return ;; esac
        input_path="$(cd "$(dirname "$MG_ROOT/$1")" && pwd -P)/$(basename "$1")" || return 1
        case "$input_path" in "$MG_ROOT/build/"*) ;; *) mg_failure input-path "request escapes build/"; mg_status error; return ;; esac
        if [ -L "$input_path" ] || [ -z "$(find "$input_path" -prune -type f -links 1 -print)" ]; then
          mg_failure input-path "request must be one owned regular file"; mg_status error; return
        fi
        mount_args=(-v "$input_path:/input/request.json:ro")
        runtime_args=(smoke --request /input/request.json)
      fi ;;
    *) if [ "$#" -ne 0 ]; then mg_failure usage "this mode takes no arguments"; mg_status error; return; fi ;;
  esac
  source_commit="$(git -C "$MG_ROOT" rev-parse HEAD)" || return 1
  source_sha="$(mg_sync_reference_digest)" || return 1
  if ! mg_compose -f "$MG_ROOT/docker/meguri-sync-reference.compose.yml" --progress plain build sync-reference; then
    mg_failure image-build "current reference image build failed"; mg_status fail; return
  fi
  if [ "$source_sha" != "$(mg_sync_reference_digest)" ]; then
    mg_failure source-changed "reference source changed during its build"; mg_status fail; return
  fi
  lock_sha="$(shasum -a 256 "$reference_dir/requirements.lock" | cut -d ' ' -f1)"
  # Explicit source authors, independent of the operator and checkout path.
  author_session="s-20261116T124044Z-c9976196"
  source_author_sessions='["s-20261115T133013Z-d8cec007","s-20261115T211543Z-556b04e9","s-20261115T224434Z-376ca308","s-20261115T231059Z-ff95725a","s-20261116T001757Z-8fe546bc","s-20261116T020051Z-5e05fa3f","s-20261116T092626Z-4487d759","s-20261116T124044Z-c9976196","s-20261116T225916Z-981c8d1b","s-20261117T230521Z-4667d769"]'
  if ! git -C "$MG_ROOT" show "${source_commit}:docs/process/provenance/sessions.jsonl" |
      jq -es --argjson authors "$source_author_sessions" '
        [.[] | select(.session as $id | $authors | index($id))] | unique |
        select(length == ($authors | length)) |
        select((map(.session) | sort) == ($authors | sort)) |
        select(all(.role == "oracle-maintainer"))' >"$MG_RUN_DIR/source-authors.json"; then
    mg_failure session-missing "explicit reference source authors require unambiguous committed oracle-maintainer records"; mg_status error; return
  fi
  mg_artifact report "$MG_RUN_DIR/source-authors.json" "Committed responsible reference source authors"
  local batch_base="38235917e6a025daa57625c582e90d253aae62e3"
  case "$mode" in recovery-dirty) batch_base="146cb76fd38c2a8ff724b837df979d25eabc71f9" ;; recovery|recovery-discover) batch_base="8506d4879dd3b76b4aa5e1ab0825409671b84549" ;; esac
  jq -n --arg batch "$batch_base" --arg commit "$source_commit" --arg source "$source_sha" --arg lock "$lock_sha" \
    --arg run "$MG_RUN_ID" --arg author "$author_session" --argjson authors "$source_author_sessions" --argjson args "$argument_json" \
    '{source_commit:$commit,batch_base:$batch,source_sha256:$source,lock_sha256:$lock,mg_job:"sync-server-reference",arguments:$args,run_id:$run,author_session:$author,source_author_sessions:$authors}' \
    >"$MG_RUN_DIR/invocation.json" || return 1
  local execution_status=0 output_dir="$MG_RUN_DIR" trusted_invocation
  if [ "$mode" = selftest ] || [ "$mode" = identity ]; then
    trusted_invocation="$(cat "$MG_RUN_DIR/invocation.json")" || return 1
    output_dir="$MG_RUN_DIR/reference-output"
    mg_sync_reference__retained "$mode" "$output_dir" || execution_status=$?
  else
    mg_compose -f "$MG_ROOT/docker/meguri-sync-reference.compose.yml" run --rm -T --no-deps \
    --label "meguri.mg.run-id=$MG_RUN_ID" -u "$(id -u):$(id -g)" \
    -v "$MG_RUN_DIR:/out" -e "MG_RUN_ID=$MG_RUN_ID" \
    ${mount_args[0]+"${mount_args[@]}"} \
      sync-reference "${runtime_args[@]}" || execution_status=$?
  fi
  if [ "$source_sha" != "$(mg_sync_reference_digest)" ] || [ "$source_commit" != "$(git -C "$MG_ROOT" rev-parse HEAD)" ]; then
    mg_failure source-changed "reference source/commit changed during execution"; execution_status=1
  fi
  if [ -f "$output_dir/summary.json" ]; then
    mg_count total "$(jq -r '.total // 0' "$output_dir/summary.json")"
    mg_count passed "$(jq -r '.passed // 0' "$output_dir/summary.json")"
    mg_count failed "$(jq -r '.failed // 0' "$output_dir/summary.json")"
    mg_detail reference_evidence "$(cat "$output_dir/summary.json")"
  fi
  if [ "$mode" = selftest ] || [ "$mode" = identity ]; then
    mg_detail reference_invocation "$trusted_invocation"
  else
    mg_detail reference_invocation "$(cat "$MG_RUN_DIR/invocation.json")"
  fi
  if [ -f "$output_dir/evidence.json" ]; then mg_artifact report "$output_dir/evidence.json" "Exact neutral observations"; fi
  mg_artifact directory "$MG_RUN_DIR" "Current-release reference evidence"
  if [ "$execution_status" -ne 0 ]; then
    mg_failure reference-execution "reference harness failed; no compatibility credit"; mg_status fail; return
  fi
  if [ "$mode" = smoke ] || [ "$mode" = edges ] || [ "$mode" = normal ] || [ "$mode" = normal-sdk ] || [ "$mode" = normal-matrix ] || [ "$mode" = media ] || [ "$mode" = recovery ]; then
    mg_message "Named Desktop reference selection passed; broad catalog and actual Mobile remain unverified"
  fi
  mg_status pass
}
