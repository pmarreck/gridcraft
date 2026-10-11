# shellcheck shell=bash disable=SC2034
# Shared implementation of ./build, ./build-all and ./test. Compatible with macOS Bash 3.2.
GC_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd) || exit 1
GC_VERSION=$(awk '/^\[workspace.package\]/{p=1;next} /^\[/{p=0} p && /^version = /{gsub(/"/, "", $3);print $3;exit}' "$GC_ROOT/Cargo.toml")
GC_UPSTREAM_URL=https://github.com/storytold/gridcraft.git
# A verified upstream sync stays valid this long before builds refresh it.
GC_SYNC_MAX_AGE=86400
GC_TARGETS="aarch64-macos aarch64-linux x86_64-linux aarch64-windows x86_64-windows"

die() { printf 'error: %s\n' "$*" >&2; exit 1; }
note() { printf '%s\n' "$*" >&2; }
nix_cmd() { nix --extra-experimental-features 'nix-command flakes' "$@"; }
git_root() { git -C "$GC_ROOT" "$@"; }
# The only clock read, so tests can substitute `date` on PATH.
now() { date +%s; }

about() {
	printf '%s %s: %s (%s/%s)\n' "$1" "$GC_VERSION" "$2" "$(uname -s)" "$(uname -m)"
}

# Normalize the executing environment (WSL selects Linux) to GC_HOST (aarch64-linux) and the Nix
# system that builds it (GC_SYSTEM, e.g. aarch64-darwin).
host_platform() {
	local os arch
	os=$(uname -s) || return 1
	arch=$(uname -m) || return 1
	case "$os" in Linux) os=linux ;; Darwin) os=macos ;; *) die "unsupported Nix host: $os" ;; esac
	case "$arch" in arm64|aarch64) arch=aarch64 ;; amd64|x86_64) arch=x86_64 ;; *) die "unsupported architecture: $arch" ;; esac
	GC_HOST="$arch-$os"
	GC_SYSTEM="$arch-$os"
	[[ $os != macos ]] || GC_SYSTEM="$arch-darwin"
}

target_info() {
	case "$1" in
		aarch64-macos) GC_TRIPLE=aarch64-apple-darwin; GC_OS=macos; GC_ARCH=aarch64 ;;
		aarch64-linux) GC_TRIPLE=aarch64-unknown-linux-gnu; GC_OS=linux; GC_ARCH=aarch64 ;;
		x86_64-linux) GC_TRIPLE=x86_64-unknown-linux-gnu; GC_OS=linux; GC_ARCH=x86_64 ;;
		aarch64-windows) GC_TRIPLE=aarch64-pc-windows-gnullvm; GC_OS=windows; GC_ARCH=aarch64 ;;
		x86_64-windows) GC_TRIPLE=x86_64-pc-windows-gnu; GC_OS=windows; GC_ARCH=x86_64 ;;
		*) die "unsupported target: $1 (one of: $GC_TARGETS)" ;;
	esac
}

sync_cache_path() { git_root rev-parse --path-format=absolute --git-path gridcraft-upstream-sync; }

# Succeeds when the recorded sync is under GC_SYNC_MAX_AGE old (and not in the future) and HEAD
# still contains the upstream commit it recorded. A marker alone is not evidence of freshness.
sync_is_fresh() {
	local path stamp commit extra t
	path=$(sync_cache_path) || return 1
	[[ -f $path ]] || return 1
	read -r stamp commit extra <"$path" || return 1
	[[ $stamp =~ ^[0-9]+$ && $commit =~ ^[0-9a-f]{40,64}$ && -z $extra ]] || return 1
	t=$(now) || return 1
	((t >= stamp && t - stamp < GC_SYNC_MAX_AGE)) || return 1
	git_root merge-base --is-ancestor "$commit" HEAD 2>/dev/null
}

# Fetch upstream main and rebase the current branch onto it, then record the sync. Refuses a
# detached HEAD, a dirty tracked tree or an unfinished Git operation; never stashes or discards.
sync_upstream() {
	local state state_path commit path
	for state in rebase-merge rebase-apply MERGE_HEAD CHERRY_PICK_HEAD REVERT_HEAD; do
		state_path=$(git_root rev-parse --path-format=absolute --git-path "$state") || return 1
		[[ ! -e $state_path ]] || die 'finish or abort the Git operation in progress before updating'
	done
	git_root symbolic-ref --quiet HEAD >/dev/null || die 'check out a branch before updating (HEAD is detached)'
	[[ -z $(git_root status --porcelain --untracked-files=no) ]] || die 'tracked files have uncommitted changes; commit them, or build with --no-update within 24 h of the last sync'
	if ! git_root config --get remote.upstream.url >/dev/null; then
		note "Adding remote upstream: $GC_UPSTREAM_URL"
		git_root remote add upstream "$GC_UPSTREAM_URL" || return 1
	fi
	note 'Fetching upstream main and rebasing onto it...'
	git_root fetch --quiet upstream main || die 'could not fetch upstream main; nothing was built'
	commit=$(git_root rev-parse --verify 'upstream/main^{commit}') || return 1
	if ! git_root rebase --quiet upstream/main; then
		note 'Rebase stopped on a conflict. Resolve it and run git rebase --continue, or restore the branch with git rebase --abort. Nothing was built.'
		return 1
	fi
	path=$(sync_cache_path) || return 1
	printf '%s %s\n' "$(now)" "$commit" >"$path.tmp.$$" && command mv -f "$path.tmp.$$" "$path"
}

# mode: auto (sync when stale), force (always sync), off (require a fresh sync, never fetch).
ensure_upstream() {
	case "$1" in
		off)
			sync_is_fresh || die 'no verified upstream sync in the last 24 h; run without --no-update (or with --update) first'
			;;
		auto)
			if sync_is_fresh; then
				note 'Upstream sync is under 24 h old; not fetching.'
			else
				sync_upstream || exit 1
			fi
			;;
		force) sync_upstream || exit 1 ;;
		*) die "unknown update mode: $1" ;;
	esac
}

# Build one target through Nix and, only after success, point bin/<os>/<arch> at its bin
# directory. Each target/profile keeps its own GC root under .nix-out.
build_target() {
	local target=$1 profile=$2 attribute link output suffix destination pending moved
	target_info "$target"
	attribute="packages.$GC_SYSTEM.$target"
	[[ $profile != debug ]] || attribute="$attribute-debug"
	link="$GC_ROOT/.nix-out/$GC_TRIPLE/$profile"
	mkdir -p "$(dirname "$link")" || return 1
	nix_cmd build "$GC_ROOT#$attribute" --out-link "$link" --print-build-logs || return 1
	output=$(cd "$link" && pwd -P) || return 1
	suffix=
	[[ $GC_OS != windows ]] || suffix=.exe
	[[ -x "$output/bin/gridcraft$suffix" && -x "$output/bin/gridcraft-cli$suffix" ]] || die "missing executables in $output/bin"
	destination="$GC_ROOT/bin/$GC_OS/$GC_ARCH"
	mkdir -p "$(dirname "$destination")" || return 1
	[[ ! -e $destination || -L $destination ]] || die "refusing to replace a real directory: $destination"
	mkdir "$destination.lock" 2>/dev/null || die "another build is publishing $destination"
	pending="$destination.new.$$"
	moved=0
	# Rename over the old selector atomically, never following the old directory link.
	if ln -s "../../.nix-out/$GC_TRIPLE/$profile/bin" "$pending"; then
		if [[ $(uname -s) == Darwin ]]; then
			command mv -fh "$pending" "$destination" && moved=1
		else
			command mv -Tf "$pending" "$destination" && moved=1
		fi
	fi
	rmdir "$destination.lock"
	((moved)) || { rm -f -- "$pending"; return 1; }
	note "Built $target ($profile): bin/$GC_OS/$GC_ARCH/gridcraft$suffix, bin/$GC_OS/$GC_ARCH/gridcraft-cli$suffix"
}
