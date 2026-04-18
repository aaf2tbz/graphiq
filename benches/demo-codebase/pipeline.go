package pipeline

import (
	"fmt"
	"strings"
	"sync"
	"time"
)

type TransformStep func(input string) (string, error)

type Pipeline struct {
	steps    []TransformStep
	mu       sync.Mutex
	metrics  map[string]int64
}

func NewPipeline(steps ...TransformStep) *Pipeline {
	return &Pipeline{
		steps:   steps,
		metrics: make(map[string]int64),
	}
}

func (p *Pipeline) Execute(input string) (string, error) {
	p.mu.Lock()
	p.metrics["total_executions"]++
	p.mu.Unlock()

	result := input
	for i, step := range p.steps {
		var err error
		result, err = step(result)
		if err != nil {
			p.mu.Lock()
			p.metrics[fmt.Sprintf("step_%d_error", i)]++
			p.mu.Unlock()
			return "", fmt.Errorf("step %d failed: %w", i, err)
		}
		p.mu.Lock()
		p.metrics[fmt.Sprintf("step_%d_success", i)]++
		p.mu.Unlock()
	}
	return result, nil
}

func (p *Pipeline) GetMetrics() map[string]int64 {
	p.mu.Lock()
	defer p.mu.Unlock()
	copy := make(map[string]int64)
	for k, v := range p.metrics {
		copy[k] = v
	}
	return copy
}

func StripWhitespace(input string) (string, error) {
	return strings.TrimSpace(input), nil
}

func ConvertToLowerCase(input string) (string, error) {
	return strings.ToLower(input), nil
}

func RemoveDuplicateLines(input string) (string, error) {
	seen := make(map[string]bool)
	var result []string
	for _, line := range strings.Split(input, "\n") {
		if !seen[line] {
			seen[line] = true
			result = append(result, line)
		}
	}
	return strings.Join(result, "\n"), nil
}

func ComputeLevenshteinDistance(a, b string) int {
	if len(a) == 0 {
		return len(b)
	}
	if len(b) == 0 {
		return len(a)
	}
	matrix := make([][]int, len(a)+1)
	for i := range matrix {
		matrix[i] = make([]int, len(b)+1)
		matrix[i][0] = i
	}
	for j := 0; j <= len(b); j++ {
		matrix[0][j] = j
	}
	for i := 1; i <= len(a); i++ {
		for j := 1; j <= len(b); j++ {
			cost := 1
			if a[i-1] == b[j-1] {
				cost = 0
			}
			matrix[i][j] = min(
				matrix[i-1][j]+1,
				matrix[i][j-1]+1,
				matrix[i-1][j-1]+cost,
			)
		}
	}
	return matrix[len(a)][len(b)]
}

func FormatTimestamp(t time.Time, layout string) string {
	return t.Format(layout)
}

func ParseBooleanString(input string) (bool, error) {
	switch strings.ToLower(strings.TrimSpace(input)) {
	case "true", "yes", "1", "on":
		return true, nil
	case "false", "no", "0", "off":
		return false, nil
	default:
		return false, fmt.Errorf("cannot parse '%s' as boolean", input)
	}
}
