package main

import "sync"

var thumbnailSem = make(chan struct{}, 2)
var thumbnailMu sync.Mutex
var thumbnailInFlight = map[string]*thumbnailWaiter{}
var thumbnailFailed = map[string]bool{}

type thumbnailWaiter struct {
	done chan struct{}
	path string
	err  error
}
