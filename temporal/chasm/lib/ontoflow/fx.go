package ontoflow

import (
	"fmt"

	"go.temporal.io/server/chasm"
	"go.temporal.io/server/common/log"
	"go.uber.org/fx"
)

// Module provides the OntoFlow library to the CHASM registry.
var Module = fx.Module(
	"ontoflow",
	fx.Invoke(func(registry *chasm.Registry, logger log.Logger) error {
		logger.Info(fmt.Sprintf("OntoFlow library registered: component=OntoFlowExecutionComponent version=v0.1"))
		return nil
	}),
)
