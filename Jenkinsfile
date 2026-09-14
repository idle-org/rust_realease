// Add this webhook in Gitea:
// http://192.168.1.37:8080/gitea-webhook/post

// Skeleton checks only; release behavior remains in the Python implementation.
pipeline {
    agent { label 'rust_no_docker' }

    environment {
        CARGO_TERM_COLOR = 'never'
    }

    stages {
        stage('Checkout') {
            steps {
                checkout scm
            }
        }

        stage('Rust Toolchain') {
            steps {
                sh '''
                    set -eu
                    rustc --version
                    cargo --version
                    cargo fmt --version
                    cargo clippy --version
                '''
            }
        }

        stage('Format') {
            steps {
                sh 'cargo fmt --check'
            }
        }

        stage('Clippy') {
            steps {
                sh 'cargo clippy --locked --offline --all-targets --all-features -- -D warnings'
            }
        }

        stage('Tests') {
            steps {
                sh 'cargo test --locked --offline --all'
            }
        }

        stage('Build') {
            steps {
                sh 'cargo build --locked --offline --release'
            }
        }

        stage('Skeleton Smoke Test') {
            steps {
                sh 'cargo run --locked --offline --release'
            }
        }
    }

    post {
        success {
            publishChecks(
                conclusion: 'SUCCESS',
                name: 'Rust CI',
                summary: 'Rust skeleton passed all CI checks.',
                text: 'Formatting, strict Clippy, Cargo tests, release build, and skeleton startup passed. Release functionality is not implemented yet.',
                title: 'CI Successful'
            )
        }

        failure {
            publishChecks(
                conclusion: 'FAILURE',
                name: 'Rust CI',
                summary: 'Rust skeleton failed CI.',
                text: 'Check the Jenkins log for toolchain, formatting, Clippy, test, build, or startup failures.',
                title: 'CI Failed'
            )
        }

        unstable {
            publishChecks(
                conclusion: 'ACTION_REQUIRED',
                name: 'Rust CI',
                summary: 'Rust skeleton CI is unstable.',
                text: 'The Jenkins build completed with an unstable status.',
                title: 'CI Unstable'
            )
        }
    }
}
