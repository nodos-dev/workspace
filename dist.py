import argparse
import io
from subprocess import CompletedProcess, call, run
import subprocess
from sys import stderr, stdout
import zipfile
from loguru import logger
import os
import shutil
import json
import pathlib
import glob
import platform
import requests

ARTIFACTS_FOLDER = "./Artifacts"
STAGING_FOLDER = f"{ARTIFACTS_FOLDER}/__staging__"

def should_exclude_from_install(target):
    INSTALLER_EXCLUDE = set(["flatc", 
                             "curl",
                             "gen_hpack_tables",
                             "protoc",
                             "grpc_cpp_plugin",
                             "Test",
                             "vkClient"
                             "zlib"
                            ])
    for exclude in INSTALLER_EXCLUDE:
        if exclude.lower() in target.lower():
            logger.debug("Excluding " + target)
            return True
    return False


def getenv(var_name):
    val = os.getenv(var_name)
    if val is None:
        logger.error(f"Environment variable {var_name} is not set!")
        exit(1)
    return val


def sign_files(files):
    # TODO: Use Nodos certificate once arrived.
    url = getenv("ZD_CODE_SIGN_AZURE_KEY_VAULT_URL")
    client_id = getenv("ZD_CODE_SIGN_AZURE_KEY_VAULT_CLIENT_ID")
    client_secret = getenv("ZD_CODE_SIGN_AZURE_KEY_VAULT_CLIENT_SECRET")
    tenant_id = getenv("ZD_CODE_SIGN_AZURE_TENANT_ID")
    cert_name = getenv("ZD_CODE_SIGN_AZURE_KEY_VAULT_CERT_NAME")

    logger.info(f"Signing {' '.join(files)}")
    args = ["AzureSignTool", "sign", 
            "-kvu", url,
            "-kvi", client_id,
            "-kvs", client_secret,
            "-kvt", tenant_id,
            "-kvc", cert_name,
            "-tr", "http://timestamp.digicert.com",
            "-v",
            *files]
    retcode = call(args)
    if retcode != 0:
        logger.error(f"Signtool returned with code {retcode}")


def sign_binaries(folder):
    logger.info(f"Signing binaries inside: {folder}")
    files_to_be_signed = []
    bin_ext = set([".exe", ".dll"])
    for root, dirs, files in os.walk(folder):
        for filename in files:
            path = pathlib.Path(f"{root}/{filename}")
            if (path.suffix in bin_ext) and not should_exclude_from_install(filename):
                files_to_be_signed.append(path.as_posix())
    sign_files(files_to_be_signed)


def run_dry_runnable(args, dry_run):
    if dry_run:
        logger.info("Dry run: %s" % " ".join(args))
        return CompletedProcess(args, 0, "", "")
    return run(args, capture_output=True, text=True, env=os.environ.copy())


def get_version_info_from_env():
    major =        os.getenv("VERSION_MAJOR")
    minor =        os.getenv("VERSION_MINOR")
    patch =        os.getenv("VERSION_PATCH")
    build_number = os.getenv('BUILD_NUMBER')
    if not major or not minor or not patch or not build_number:
        logger.error("Missing version info. Make sure to set VERSION_MAJOR, VERSION_MINOR, VERSION_PATCH and BUILD_NUMBER")
        exit(1)
    return major, minor, patch, build_number


def parse_version(filepath, version_define_prefix):
    with open(filepath, "r") as f:
        major = None
        minor = None
        patch = None
        for line in f.readlines():
            # 👏👏👏
            if line.startswith(f"#define {version_define_prefix}_VERSION_MAJOR"):
                major = int(line.split(" ")[-1])
            elif line.startswith(f"#define {version_define_prefix}_VERSION_MINOR"):
                minor = int(line.split(" ")[-1])
            elif line.startswith(f"#define {version_define_prefix}_VERSION_PATCH"):
                patch = int(line.split(" ")[-1])
        if major is None or minor is None or patch is None:
            logger.error(f"Failed to parse {version_define_prefix}_VERSION")
            exit(1)
        return {"major": major, "minor": minor, "patch": patch}


def create_nodos_release(gh_release_repo, gh_release_notes, gh_release_title_postfix, gh_release_target_branch, dry_run_release, skip_nosman_publish):
    release_repo, release_notes, title_postfix, target_branch = gh_release_repo, gh_release_notes, gh_release_title_postfix, gh_release_target_branch
    artifacts = get_release_artifacts(ARTIFACTS_FOLDER)
    for path in artifacts:
        logger.info(f"Release artifact: {path}")
    major, minor, patch, build_number = get_version_info_from_env()
    tag = f"v{major}.{minor}.{patch}.b{build_number}"
    title = f"{tag}{title_postfix}"
    ghargs = ["gh", "release", "create", tag, *artifacts, "--notes", f"{release_notes}", "--title", title]
    if target_branch != "":
        logger.info(f"GitHub Release: Using target branch {target_branch}")
        ghargs.extend(["--target", target_branch])
    else:
        logger.info("GitHub Release: Using default branch")
    if release_repo != "":
        logger.info(f"GitHub Release: Using repo {release_repo}")
        ghargs.extend(["--repo", release_repo])
    else:
        logger.info("GitHub Release: The repo inside the current directory will be used with '--generate-notes' option")
        ghargs.extend(["--generate-notes"])
    logger.info(f"GitHub Release: Pushing release artifacts to repo {release_repo}")
    result = run_dry_runnable(ghargs, dry_run_release)
    if result.returncode != 0:
        print(result.stderr)
        logger.error(f"GitHub CLI returned with {result.returncode}")
        exit(result.returncode)
    logger.info("GitHub release successful")
    if skip_nosman_publish:
        return

    version = f"{major}.{minor}.{patch}.b{build_number}"
    nodos_zip_prefix = f"Nodos-{version}"
    sdk_zip_prefix = f"Nodos-SDK-{version}"

    artifacts_abspath = [os.path.abspath(path) for path in artifacts]
    
    cwd = os.getcwd()
    os.chdir(STAGING_FOLDER)
    for path in artifacts_abspath:
        abspath = os.path.abspath(path)
        file_name = os.path.basename(path)
        if not file_name.startswith(nodos_zip_prefix):
            continue
        # If file_name is of format Nodos-{major}.{minor}.{patch}.b{build_number}-bundle-{dist_key}.zip, it is a bundled distribution. Get the dist_key from it.
        dist_key = None
        if file_name.startswith(f"{nodos_zip_prefix}-bundle-"):
            dist_key = file_name.split("-bundle-")[1].split(".zip")[0]
        # Use nosman to publish Nodos:
        logger.info("Running nosman publish")
        nodos_package_name = f"nodos{f'.bundle.{dist_key}' if dist_key is not None else ''}"
        nosman_args = [f"nosman", "publish", "--path", path, "--name", nodos_package_name, "--version", f"{major}.{minor}.{patch}", "--version-suffix", f".b{build_number}", "--type", "nodos", "--vendor", "Nodos", "--publisher-name", "nosman", "--publisher-email",
                    "bot@nodos.dev"]
        if dry_run_release:
            nosman_args.append("--dry-run")
        logger.info(f"Running nosman publish with args: {nosman_args}")
        result = run(nosman_args, stdout=stdout, stderr=stderr, universal_newlines=True)
        if result.returncode != 0:
            logger.error(f"nosman publish returned with {result.returncode}")
            exit(result.returncode)
    os.chdir(cwd)


def get_release_artifacts(dir):
    files = glob.glob(f"{dir}/*.zip")
    return files


def get_plugin_api_version(engine_folder):
    nos_plugin_api_h = os.path.join(engine_folder, "SDK", "include", "Nodos", "PluginAPI.h")
    return parse_version(nos_plugin_api_h, "NOS_PLUGIN_API")


def get_subsystem_api_version(engine_folder):
    nos_subsystem_api_h = os.path.join(engine_folder, "SDK", "include", "Nodos", "SubsystemAPI.h")
    return parse_version(nos_subsystem_api_h, "NOS_SUBSYSTEM_API")


def download_module_packages(engine_folder, distconf, all, target_dir, profile_json_path):
    os.makedirs(target_dir, exist_ok=True)
    # TODO: Use nosman

    # TODO: Include module versions so that we can verify they can work together.
    bundled_modules = set(distconf["bundled_modules"] if "bundled_modules" in distconf else [])
    if "includes" in distconf:
        for include in distconf["includes"]:
            for other_conf in all:
                if include == other_conf["key"]:
                    others = set(other_conf["bundled_modules"] if "bundled_modules" in other_conf else [])
                    logger.debug(f"Adding modules from {other_conf['key']}: {', '.join(others)}")
                    bundled_modules.update(others)

    logger.info(f"Downloading modules: {', '.join(bundled_modules)}")
    index_url = distconf["module_index_url"]
    logger.info(f"Downloading module directory from {index_url}")
    index = requests.get(index_url).json()
    subsys_api_version = get_subsystem_api_version(engine_folder)
    plugin_api_version = get_plugin_api_version(engine_folder)
    logger.info(f"Current plugin API version: {plugin_api_version['major']}.{plugin_api_version['minor']}.{plugin_api_version['patch']}")
    logger.info(f"Current subsystem API version: {subsys_api_version['major']}.{subsys_api_version['minor']}.{subsys_api_version['patch']}")
    cur_api_ver = {"plugin": plugin_api_version, "subsystem": subsys_api_version}
    included_modules = []
    for module in index:
        name = module['name']
        if name not in bundled_modules:
            logger.debug(f"Skipping module {name}, since it is not in distribution config '{distconf['key']}'")
            continue
        production_version = module['production_version'] if 'production_version' in module else None
        module_index = requests.get(module['url']).json()
        # Latest release is at the top of module_index['releases']
        suitable_release = None
        # If production version is specified, use it, if not use the latest release that supports the current API major version
        module_type = module['type'].lower()
        if production_version is not None:
            logger.debug(f"Production version specified for module {name}, using release {production_version}")
            for release in module_index['releases']:
                if release['version'] == production_version:
                    suitable_release = release
                    break
            if suitable_release is not None and (f"{module_type}_api_version" not in suitable_release or suitable_release[f"{module_type}_api_version"]["major"] != cur_api_ver[module_type]["major"]):
                logger.warning(f"Production version {production_version} of {module_type} {name} does not support {module_type} API version {cur_api_ver[module_type]['major']}, will use latest release that supports it")
                suitable_release = None
        if suitable_release is None:
            logger.debug(f"Using latest release that supports {module_type} API version {cur_api_ver[module_type]['major']}")
            for release in module_index['releases']:
                if f"{module_type}_api_version" not in release or release[f'{module_type}_api_version']['major'] == cur_api_ver[module_type]['major']:
                    suitable_release = release
                    break
        if suitable_release is None:
            logger.warning(f"{module_type} {name} does not support {module_type} API version {cur_api_ver[module_type]['major']}, skipping")
            continue
        included_modules.append({"name": name, "version": suitable_release['version']})
        zip_url = suitable_release['url']
        version = suitable_release['version']
        zip_file_name = os.path.basename(zip_url)
        if "local_modules_folder" in distconf:
            if os.path.exists(f"{distconf['local_modules_folder']}/{zip_file_name}"):
                logger.info(f"Found {name}-{version} in {distconf['local_modules_folder']}")
                with open(f"{distconf['local_modules_folder']}/{zip_file_name}", "rb") as f:
                    with zipfile.ZipFile(f) as z:
                        z.extractall(f"{target_dir}/{name}/{version}")
                continue
        # Download zip
        logger.info(f"Downloading {module_type} {name}:{version} from {zip_url}")
        zip_file = requests.get(zip_url)
        # Unzip to target_dir/name/version
        with zipfile.ZipFile(io.BytesIO(zip_file.content)) as z:
            z.extractall(f"{target_dir}/{name}/{version}")
    # Write included modules to Profile.json
    profile = {}
    if os.path.exists(profile_json_path):
        with open(f"{profile_json_path}", "r") as f:
            profile = json.load(f)
    if "loaded_modules" not in profile:
        profile["loaded_modules"] = []
    profile["loaded_modules"].extend(included_modules)
    with open(f"{profile_json_path}", "w") as f:
        json.dump(profile, f, indent=2)


def build_nosman(nosman_src_dir, is_release) -> str:
    logger.info("Building nosman")
    cwd = os.getcwd()
    os.chdir(nosman_src_dir)
    result = run(["cargo", "build", "--release" if is_release else "--debug"], stdout=stdout, stderr=stderr, universal_newlines=True)
    if result.returncode != 0:
        logger.error("Failed to build nosman")
        exit(result.returncode)
    # Compress with upx (optional)
    result = run(["upx", "--best", f"target/{'release' if is_release else 'debug'}/nosman{platform.system() == 'Windows' and '.exe' or ''}"], stdout=stdout, stderr=stderr, universal_newlines=True)
    if result.returncode != 0:
        logger.warning("Failed to compress nosman with upx, skipping")
    os.chdir(cwd)
    return f"{nosman_src_dir}/target/{'release' if is_release else 'debug'}/nosman{platform.system() == 'Windows' and '.exe' or ''}"


def remove_dir(d):
    if os.name == 'nt':
        subprocess.check_output(['cmd', '/C', 'rmdir', '/S', '/Q', os.path.abspath(d)])
    else:
        subprocess.check_output(['rm', '-rf', os.path.abspath(d)])


def package(dist_key, engine_folder, should_sign_binaries):
    logger.info("Packaging Nodos")
    shutil.rmtree(ARTIFACTS_FOLDER, ignore_errors=True)
    os.makedirs(f"{STAGING_FOLDER}", exist_ok=True)
    dists = []
    with open("./bundle.json", 'r') as f:
        dists = json.load(f)
    for dist in dists:
        if dist['key'] == dist_key:
            distconf = dist
            break
    else:
        logger.error(f"Dist key {dist_key} not found in dist.json")
        exit(1)

    is_bundled = "bundled_modules" in distconf and len(distconf["bundled_modules"]) > 0

    leftovers = get_release_artifacts(".")
    for path in leftovers:
        logger.info(f"Deleting {path}")
        os.remove(path)

    bin_dir = str(pathlib.Path(f"{engine_folder}/Binaries").absolute())
    sdk_dir = str(pathlib.Path(f"{engine_folder}/SDK").absolute())
    # Create a nosSDK-v<version>.zip file from SDK folder
    logger.info(f"Creating SDK zip file")
    major, minor, patch, build_number = get_version_info_from_env()
    sdk_zip_name = f"Nodos-SDK-{major}.{minor}.{patch}.b{build_number}.zip"

    # Copy nosman
    nosman_path = build_nosman(f"./Toolchain/nosman", is_release=True)
    os.makedirs(f"{STAGING_FOLDER}", exist_ok=True)
    shutil.copy(nosman_path, f"{STAGING_FOLDER}/nosman{platform.system() == 'Windows' and '.exe' or ''}")
    # TODO: Copy source

    # Copy CMakes
    shutil.copytree(f"./Toolchain/CMake", f"{STAGING_FOLDER}/Toolchain/CMake")

    # Copy Binaries, Config and SDK folder to Staging folder
    engine_dist_folder = f"{STAGING_FOLDER}/Engine/{major}.{minor}.{patch}.b{build_number}"
    os.makedirs(engine_dist_folder, exist_ok=True)
    os.makedirs(f"{STAGING_FOLDER}/Module", exist_ok=True)

    # SDK
    shutil.copytree(sdk_dir, f"{engine_dist_folder}/SDK")
    
    # Call nosman init
    cwd = os.getcwd()
    os.chdir(STAGING_FOLDER)
    logger.info("Running nosman init")
    result = run(["nosman", "init"], stdout=stdout, stderr=stderr, universal_newlines=True)
    if result.returncode != 0:
        logger.error(f"nosman init returned with {result.returncode}")
        exit(result.returncode)
    os.chdir(cwd)

    remove_dir(f"{STAGING_FOLDER}/.nosman/remote")

    # Zip SDK only release
    sdk_zip_name = f"Nodos-SDK-{major}.{minor}.{patch}.b{build_number}"
    shutil.make_archive(f"./Artifacts/{sdk_zip_name}", 'zip', f"{STAGING_FOLDER}")

    # Move rest
    shutil.copytree(bin_dir, f"{engine_dist_folder}/Binaries",)
    shutil.copytree(f"{engine_folder}/Config", f"{engine_dist_folder}/Config")
    shutil.copy(f"{engine_folder}/EULA_UNCONFIRMED.json", f"{engine_dist_folder}/EULA_UNCONFIRMED.json")

    if should_sign_binaries:
        sign_binaries(f"{engine_dist_folder}/Binaries")
        sign_binaries(f"{engine_dist_folder}/SDK/bin")

    if is_bundled:
        download_module_packages(engine_folder, distconf, dists, f"{STAGING_FOLDER}/Module", f"{engine_dist_folder}/Config/Profile.json")

    engine_settigns_path = f"{engine_dist_folder}/Config/EngineSettings.json"
    with open(engine_settigns_path, "r") as f:
        engine_settings = json.load(f)
        engine_settings["remote_modules"] = [{"url": distconf["module_index_url"]}]
        engine_settings["engine_index_url"] = distconf["engine_index_url"]

    with open(engine_settigns_path, "w") as f:
        json.dump(engine_settings, f, indent=2)

    # Zip everything under staging
    shutil.make_archive(f"./Artifacts/Nodos-{major}.{minor}.{patch}.b{build_number}{f'-bundle-{dist_key}' if is_bundled else ''}", 'zip', f"{STAGING_FOLDER}")


if __name__ == "__main__":
    logger.remove()
    logger.add(stdout, format="<green>[Distribute Nodos]</green> <level>{time:HH:mm:ss.SSS}</level> <level>{level}</level> <level>{message}</level>")

    parser = argparse.ArgumentParser(
        description="Create distribution packages for Nodos")
    parser.add_argument("--dist-key", help="The key of the distribution to package",
                        action="store")
    parser.add_argument("--engine-folder", help="The folder where the engine is built",
                        action="store")

    parser.add_argument('--gh-release',
                        action='store_true',
                        default=False,
                        help="Create a GitHub release with the installer executables")

    parser.add_argument('--gh-release-notes',
                        action='store',
                        default='',
                        help="Description of the release")

    parser.add_argument('--gh-release-repo',
                        action='store',
                        default='',
                        help="The repo of the release. If empty, the repo of the current directory will be used with '--generate-notes' option of the GitHub CLI.")

    parser.add_argument('--gh-release-title-postfix',
                        action='store',
                        default='',
                        help="Postfix to add to the release title")

    parser.add_argument('--gh-release-target-branch',
                        action='store',
                        default='',
                        help="The branch to create the release on. If empty, the current branch will be used.")

    parser.add_argument('--dry-run-release',
                        action='store_true',
                        default=False)
    
    parser.add_argument('--skip-nosman-publish',
                        action='store_true',
                        default=False)
    
    parser.add_argument('--pack',
                        action='store_true',
                        default=False,
                        help="Create a zip file for the distribution")

    args = parser.parse_args()

    if args.pack:
        package(args.dist_key, args.engine_folder, False)

    if args.gh_release:
        create_nodos_release(args.gh_release_repo, args.gh_release_notes, args.gh_release_title_postfix, args.gh_release_target_branch, args.dry_run_release, args.skip_nosman_publish)
