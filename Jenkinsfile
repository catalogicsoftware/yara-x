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
    String publishDir = "${env.SRC_DIR}/publish-linux"
    sh "mkdir -p ${publishDir}"

    String buildImage = "yara-builder-${getCommitSha()}"
    sh "docker build -t \"${buildImage}\" ${env.SRC_DIR}"
    docker.image(buildImage).inside {
        sh "cp /out/libyara_x_capi.so ${publishDir}"
    }

    uploadToNexus("${env.SRC_DIR}/publish-linux", "linux")
}

void buildForWindows()  {
    String publishDir = "${env.SRC_DIR}/publish-windows"
    sh "mkdir -p ${publishDir}"

    withCommonNodeOptions('windows2019', 1) {
        powershell "(Test-Path 'yara-x/') -and (rm -recurse yara-x)"
        powershell "git clone https://github.com/catalogicsoftware/yara-x"
        powershell "cargo cbuild -p yara-x-capi --release --target x86_64-pc-windows-msvc --target-dir yara-x/artifacts --manifest-path yara-x/Cargo.toml"
        powershell "cp yara-x/artifacts/x86_64-pc-windows-msvc/release/yara_x_capi.dll ${publishDir}"
        powershell "rm -recurse yara-x"
    }

    uploadToNexus("${env.SRC_DIR}/publish-windows", "windows")
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