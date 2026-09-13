// Add this webhook in Gitea:
// http://192.168.1.37:8080/gitea-webhook/post

pipeline {
    agent { label 'rust_no_docker' }

    environment {
        VENV = '.venv'

        // Make Python output immediately visible in Jenkins.
        PYTHONUNBUFFERED = '1'

        // Ensure the local worker package is imported.
        PYTHONPATH = "${WORKSPACE}"
    }

    stages {
        stage('Checkout') {
            steps {
                checkout scm

                sh '''
                    set -eu

                    git submodule sync --recursive
                    git submodule update --init --recursive
                '''
            }
        }

        stage('Setup Python') {
            steps {
                sh '''
                    set -eu

                    # ---------------------------------------------------------
                    # uv
                    # ---------------------------------------------------------

                    if ! command -v uv >/dev/null 2>&1; then
                        echo "uv not found — installing it..."

                        if ! command -v cargo-binstall >/dev/null 2>&1; then
                            echo "cargo-binstall not found — installing it..."
                            cargo install cargo-binstall
                        fi

                        cargo binstall --no-confirm uv
                    else
                        echo "uv already installed."
                    fi

                    echo
                    echo "uv:"
                    uv --version

                    # ---------------------------------------------------------
                    # Python virtual environment
                    # ---------------------------------------------------------

                    rm -rf "${VENV}"

                    echo
                    echo "Creating Python virtual environment..."

                    uv venv \
                        --python 3.11 \
                        "${VENV}"

                    echo
                    echo "Installing dependencies..."

                    uv pip install \
                        --python "${VENV}/bin/python" \
                        -r requirements.txt

                    echo
                    echo "Python:"
                    "${VENV}/bin/python" --version

                    echo
                    echo "Installed dependencies:"
                    uv pip list \
                        --python "${VENV}/bin/python"
                '''
            }
        }

        stage('Python Syntax') {
            steps {
                sh '''
                    set -eu

                    echo "Checking Python syntax..."

                    "${VENV}/bin/python" \
                        -m compileall \
                        -q \
                        worker \
                        tests

                    echo "Python syntax OK."
                '''
            }
        }

        stage('Shell Syntax') {
            steps {
                sh '''
                    set -eu

                    echo "Checking release.sh syntax..."

                    bash -n release.sh

                    echo "Shell syntax OK."
                '''
            }
        }

        stage('CLI Smoke Test') {
            steps {
                sh '''
                    set -eu

                    echo "Testing worker CLI import..."

                    "${VENV}/bin/python" \
                        -P \
                        -m worker \
                        --help >/dev/null

                    echo "Testing release.sh wrapper..."

                    VIRTUAL_ENV="${WORKSPACE}/${VENV}" \
                        ./release.sh --help >/dev/null

                    echo "CLI smoke tests passed."
                '''
            }
        }

        stage('Tests') {
            steps {
                sh '''
                    set -eu

                    echo "Running rust_realease test suite..."
                    echo

                    "${VENV}/bin/python" \
                        -m unittest discover \
                        -s tests \
                        -p 'test_*.py' \
                        -v
                '''
            }
        }

        stage('Repository Sanity') {
            steps {
                sh '''
                    set -eu

                    echo "Checking expected project files..."

                    for file in \
                        release.sh \
                        requirements.txt \
                        config.toml \
                        worker/__init__.py \
                        worker/__main__.py
                    do
                        if [ ! -e "${file}" ]; then
                            echo "ERROR: expected file is missing: ${file}" >&2
                            exit 1
                        fi
                    done

                    if [ ! -x release.sh ]; then
                        echo "ERROR: release.sh is not executable." >&2
                        exit 1
                    fi

                    echo "Repository sanity checks passed."
                '''
            }
        }
    }

    post {
        success {
            publishChecks(
                conclusion: 'SUCCESS',
                name: 'Python CI',
                summary: 'rust_realease passed all CI checks.',
                text: 'Python syntax, shell syntax, CLI smoke tests, repository checks, and the complete unittest suite passed.',
                title: 'CI Successful'
            )

            echo '✅ rust_realease passed all CI checks.'
        }

        failure {
            publishChecks(
                conclusion: 'FAILURE',
                name: 'Python CI',
                summary: 'rust_realease failed CI.',
                text: 'Python syntax, shell validation, CLI startup, repository sanity checks, or tests failed. Check the Jenkins log.',
                title: 'CI Failed'
            )

            echo '❌ rust_realease CI failed.'
        }

        unstable {
            publishChecks(
                conclusion: 'ACTION_REQUIRED',
                name: 'Python CI',
                summary: 'rust_realease CI is unstable.',
                text: 'The Jenkins build completed with an unstable status.',
                title: 'CI Unstable'
            )

            echo '⚠️ rust_realease CI is unstable.'
        }

        always {
            sh '''
                echo "Cleaning CI environment..."
                rm -rf .venv
                find . -type d -name '__pycache__' -prune -exec rm -rf {} + 2>/dev/null || true
            '''
        }
    }
}