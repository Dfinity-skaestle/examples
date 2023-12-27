#!/bin/bash

set -euox pipefail

function error() {
    echo "$1"
    exit 1
}

./deploy.sh || error "Deploy failed"
./test_good.sh || error "Test good failed"
./test_race.sh || error "Test race failed"
./test_timer.sh || error "Test timer failed"
echo "-------------------"
echo "Tests passed"