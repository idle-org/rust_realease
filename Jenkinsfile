// Add this webhook in Gitea:
// http://192.168.1.37:8080/gitea-webhook/post

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

        stage('Cargo Tests') {
            steps {
                sh 'cargo test --locked --offline --all'
            }
        }
    }

    post {
        success {
            publishChecks(
                conclusion: 'SUCCESS',
                name: 'Rust CI',
                summary: 'Cargo tests passed.',
                text: 'Rust tests passed without cross compilation.',
                title: 'CI Successful'
            )
        }

        failure {
            publishChecks(
                conclusion: 'FAILURE',
                name: 'Rust CI',
                summary: 'Cargo tests failed.',
                text: 'Check the Jenkins log for the Cargo test failure.',
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
    }
}
