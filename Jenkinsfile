// Add this webhook in Gitea:
// http://192.168.1.37:8080/gitea-webhook/post

pipeline {
    agent { label 'rust_no_docker' }

    environment {
        CARGO_TERM_COLOR = 'never'
        CARGO_TARPAULIN_VERSION = '0.37.5'
        BADGE_PUBLISH_ROOT = '/mnt/assets/coverage'
    }

    stages {
        stage('Checkout') {
            steps {
                checkout scm
            }
        }

        stage('Cargo Tests') {
            steps {
                sh '''
                    set -eu

                    cargo test --locked --all
                    mkdir -p coverage
                    : > "coverage/.tests-passed-${BUILD_NUMBER}"
                '''
            }
        }

        stage('Coverage') {
            steps {
                sh '''
                    set -eu

                    installed_tarpaulin=$(cargo tarpaulin --version 2>/dev/null || true)
                    if [ "${installed_tarpaulin}" != "cargo-tarpaulin-tarpaulin ${CARGO_TARPAULIN_VERSION}" ]; then
                        cargo install cargo-tarpaulin --locked --version "${CARGO_TARPAULIN_VERSION}"
                    fi

                    cargo tarpaulin \
                        --config tarpaulin.toml \
                        --out Json \
                        --output-dir coverage

                    mv coverage/tarpaulin-report.json coverage/summary.json
                    coverage_percent=$(sed -n \
                        's/^.*],"coverage":\\([0-9][0-9.]*\\),"covered":[0-9][0-9]*,"coverable":[0-9][0-9]*}$/\\1/p' \
                        coverage/summary.json)
                    scripts/render-coverage-badge.sh \
                        "${coverage_percent}" \
                        coverage/coverage.svg
                    rm coverage/summary.json
                    : > "coverage/.coverage-ready-${BUILD_NUMBER}"
                '''
            }
        }
    }

    post {
        success {
            publishChecks(
                conclusion: 'SUCCESS',
                name: 'Rust CI',
                summary: 'Cargo tests, coverage, and badge generation passed.',
                text: 'Rust tests and the enforced line-coverage gate passed without cross compilation; current badges were generated.',
                title: 'CI Successful'
            )
        }

        failure {
            publishChecks(
                conclusion: 'FAILURE',
                name: 'Rust CI',
                summary: 'Cargo tests, coverage, or badge generation failed.',
                text: 'Check the Jenkins log for the Rust test, coverage, or badge-generation failure.',
                title: 'CI Failed'
            )
        }

        unstable {
            publishChecks(
                conclusion: 'ACTION_REQUIRED',
                name: 'Rust CI',
                summary: 'Rust CI is unstable.',
                text: 'The Cargo test build completed with an unstable status.',
                title: 'CI Unstable'
            )
        }

        cleanup {
            sh '''
                set -eu

                mkdir -p coverage
                test_status=failing
                if [ -f "coverage/.tests-passed-${BUILD_NUMBER}" ]; then
                    test_status=passing
                fi
                scripts/render-test-badge.sh \
                    "${test_status}" \
                    coverage/test.svg

                if [ ! -f "coverage/.coverage-ready-${BUILD_NUMBER}" ]; then
                    scripts/render-coverage-badge.sh - coverage/coverage.svg
                fi

                package_version=$(awk '
                    $0 == "[package]" { in_package = 1; next }
                    substr($0, 1, 1) == "[" { in_package = 0 }
                    in_package && $1 == "version" && $2 == "=" {
                        gsub(/^"|"$/, "", $3)
                        print $3
                        exit
                    }
                ' Cargo.toml)
                if [ -z "${package_version}" ]; then
                    echo "could not determine package version" >&2
                    exit 2
                fi
                scripts/render-version-badge.sh \
                    "${package_version}" \
                    coverage/version.svg

                rm -f \
                    coverage/summary.json \
                    coverage/tarpaulin-report.json \
                    "coverage/.tests-passed-${BUILD_NUMBER}" \
                    "coverage/.coverage-ready-${BUILD_NUMBER}"
            '''

            archiveArtifacts(
                allowEmptyArchive: false,
                artifacts: 'coverage/coverage.svg,coverage/test.svg,coverage/version.svg',
                fingerprint: true
            )

            sh '''
                set -eu

                if [ "${BRANCH_NAME:-}" = "main" ]; then
                    scripts/publish-badges.sh \
                        "${BADGE_PUBLISH_ROOT}" \
                        coverage \
                        test \
                        version
                fi
            '''
        }
    }
}
