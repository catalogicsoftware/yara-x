@Library("dpx-jenkins-pipeline-library@master") _

void uploadToNexus(String dirName, String platformName) {
    runNexusUpload(
        fileFormat: 'RAW',
        sourceFileGlob: "${dirName}/*",
        repoName: "yara-x-RAW-RELEASE",
        repoPath: "yara-x-capi/${platformName}",
        skipManifestUpdate: true
    )
}

void buildForLinux() {
    withCommonNodeOptions("docker", 1) {
        runCheckout()

        String publishDir = "publish-linux"
        sh "mkdir -p ${publishDir}"

        String buildImage = "yara-builder"
        sh "docker build -t \"${buildImage}\" ."
        docker.image(buildImage).inside {
            sh "cp /out/libyara_x_capi.so ${publishDir}/libyara_x_capi.${env.TAG_NAME}.so"
        }

        uploadToNexus("publish-linux", "linux")
    }
}

void buildForWindows()  {
    withCommonNodeOptions('windows2019', 1) {
        checkout scm

        powershell "cargo cbuild -p yara-x-capi --release --target x86_64-pc-windows-msvc --target-dir yara-x/artifacts"

        String publishDir = "publish-windows"
        powershell "mkdir ${publishDir} -ea 0"
        powershell "cp yara-x/artifacts/x86_64-pc-windows-msvc/release/yara_x_capi.dll ${publishDir}/yara_x_capi.${env.TAG_NAME}.dll"

        uploadToNexus("publish-windows", "windows")
    }
}

// Ignore non-release builds & 'go' releases
if(! env.TAG_NAME || env.TAG_NAME.startsWith("go/")) {
    echo "Skipping build - not a matching release tag"
    return
}

stage('Build') {
    parallel([
        "linux": { 
            stage("Linux") { 
                buildForLinux() 
            } 
        },
        "windows": {
            stage("Windows") {
                buildForWindows()
            }
        }
    ])
}
