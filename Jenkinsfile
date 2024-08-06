@Library("dpx-jenkins-pipeline-library@master") _

void uploadToNexus(String dirName, String platformName) {
    runNexusUpload(
        fileFormat: 'RAW',
        sourceFileGlob: "${dirName}/*",
        // TODO: Use separate repo for YARA artifacts?
        repoName: "guardmode-agent-RAW",
        repoPath: "yara-x-capi/${platformName}",
        skipManifestUpdate: true
    )
}

void buildForLinux() {
    String publishDir = "publish-linux"
    sh "mkdir -p ${publishDir}"

    String buildImage = "yara-builder"
    sh "docker build -t \"${buildImage}\" --arg TAG=${env.TAG_NAME} ."
    docker.image(buildImage).inside {
        sh "cp /out/libyara_x_capi.so ${publishDir}/libyara_x_capi.${env.TAG_NAME}.so"
    }

    uploadToNexus("publish-linux", "linux")
}

void buildForWindows()  {
    String publishDir = "publish-windows"
    powershell "mkdir ${publishDir} -ea 0"

    withCommonNodeOptions('windows2019', 1) {
        powershell "(Test-Path 'yara-x/') -and (rm -recurse yara-x)"
        powershell "git clone https://github.com/catalogicsoftware/yara-x"
        powershell "cd yara-x; git fetch --all --tags; git checkout ${env.TAG_NAME}"
        powershell "cargo cbuild -p yara-x-capi --release --target x86_64-pc-windows-msvc --target-dir yara-x/artifacts --manifest-path yara-x/Cargo.toml"
        powershell "cp yara-x/artifacts/x86_64-pc-windows-msvc/release/yara_x_capi.dll ${publishDir}/yara_x_capi.${env.TAG_NAME}.dll"
        powershell "rm -recurse yara-x"
    }

    uploadToNexus("publish-windows", "windows")
}

// TODO: How to do this???
if(! env.TAG_NAME) {
    return
}

// Ignore 'go' releases
if(env.TAG_NAME.startsWith("go/")) {
    return
}

withCommonNodeOptions("docker", 1) {

    def parallelBuildStageList = [:]
    parallelBuildStageList["linux"] = { 
        stage("Linux") { 
            buildForLinux() 
        } 
    }
    parallelBuildStageList["windows"] = {
        stage("Windows") {
            buildForWindows()
        }
     }

    stage('Build') {
        parallel(parallelBuildStageList)
    }
}