use crate::infer::*;
use crate::internal::*;

use tract_core::ops::array::TypedReshape;

#[derive(Debug, Clone, new, Default, Hash)]
pub struct Reshape {}

tract_linalg::impl_dyn_hash!(Reshape);

impl Op for Reshape {
    fn name(&self) -> Cow<str> {
        "Reshape".into()
    }

    op_hir!();
    not_a_typed_op!();
    not_a_pulsed_op!();
}

impl StatelessOp for Reshape {
    fn eval(&self, mut inputs: TVec<Arc<Tensor>>) -> TractResult<TVec<Arc<Tensor>>> {
        let (input, shape) = args_2!(inputs);
        let shape = shape.cast_to::<TDim>()?;
        let shape = shape.as_slice::<TDim>()?;
        let input_shape = input.shape().iter().map(|d| d.to_dim()).collect::<TVec<_>>();
        let oshape = compute_shape(&input_shape, &shape)?
            .iter()
            .map(|d| d.to_integer().map(|d| d as _))
            .collect::<TractResult<TVec<_>>>()?;
        unsafe { Ok(tvec![input.into_tensor().into_shape(&*oshape)?.into_arc_tensor()]) }
    }
}

impl InferenceRulesOp for Reshape {
    fn rules<'r, 'p: 'r, 's: 'r>(
        &'s self,
        s: &mut Solver<'r>,
        inputs: &'p [TensorProxy],
        outputs: &'p [TensorProxy],
    ) -> InferenceResult {
        s.equals(&outputs[0].datum_type, &inputs[0].datum_type)?;
        s.given_2(&inputs[0].shape, &inputs[1].value, move |s, ishape, shape| {
            let shape = shape.cast_to::<TDim>()?;
            let shape = shape.as_slice::<TDim>()?;
            let oshape = compute_shape(&ishape, &shape)?;
            s.equals(&outputs[0].shape, ShapeFactoid::from(oshape))
        })
    }

    fn to_typed(
        &self,
        _source: &InferenceModel,
        node: &InferenceNode,
        target: &mut TypedModel,
        mapping: &HashMap<OutletId, OutletId>,
    ) -> TractResult<TVec<OutletId>> {
        if let Some(ref shape) = target.outlet_fact(mapping[&node.inputs[1]])?.konst {
            let input_shape: TVec<TDim> =
                target.outlet_fact(mapping[&node.inputs[0]])?.shape.to_tvec();
            let shape = shape.cast_to::<TDim>()?;
            let shape = shape.as_slice::<TDim>()?;
            let shape = compute_shape(&input_shape, shape)?;
            let op = TypedReshape::new(shape);
            return target.wire_node(&*node.name, op, [mapping[&node.inputs[0]]].as_ref());
        }
        bail!("shape input is variable")
    }

    as_op!();
}

fn compute_shape(input: &[TDim], shape_spec: &[TDim]) -> TractResult<TVec<TDim>> {
    let mut shape: TVec<TDim> = shape_spec.into();

    // deal with zeros, stop if we see a -1
    fn deal_with_zero<'a>(
        mut input_dims: std::iter::Peekable<impl Iterator<Item = &'a TDim>>,
        shape: &mut [TDim],
    ) -> TractResult<()> {
        let mut remaining_dim_input = 1.to_dim();
        for slot in shape.iter_mut() {
            if *slot == (-1).into() {
                break;
            }
            if *slot == 0.into() {
                if remaining_dim_input != TDim::one() {
                    bail!("Invalid");
                }
                *slot = input_dims.peek().ok_or("Invalid")?.clone().clone();
            }
            loop {
                let quotient = remaining_dim_input.maybe_div(slot);
                if quotient.is_err() || quotient.as_ref().unwrap().1 != 1 {
                    remaining_dim_input =
                        remaining_dim_input.maybe_mul(input_dims.next().ok_or("Invalid")?)?;
                } else {
                    break;
                }
            }
            remaining_dim_input = remaining_dim_input.maybe_div(&slot)?.0;
        }
        Ok(())
    }

    deal_with_zero(input.iter().peekable(), &mut shape)?;
    shape.reverse();
    deal_with_zero(input.iter().rev().peekable(), &mut shape)?;
    shape.reverse();

    if let Some(pos) = shape.iter().position(|d| *d == (-1).into()) {
        let input_vol = input.iter().try_fold(1.to_dim(), |a, b| a.maybe_mul(b))?;
        let shape_vol = shape
            .iter()
            .filter(|d| **d != (-1).into())
            .try_fold(1.to_dim(), |a, b| a.maybe_mul(b))?;
        let div = input_vol.maybe_div(&shape_vol)?;
        if div.1 != 1 {
            bail!("invalid")
        }
        shape[pos] = div.0;
    }
    Ok(shape)
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! s {
        ($($a:expr),*) => {&[ $($a.into()),* ]}
    }

    #[test]
    fn reshape_invalid() {
        assert!(compute_shape(s![3, 4, 5], s!(100)).is_err());
    }

    #[test]
    fn reshape_with_leading_zero() {
        assert_eq!(&*compute_shape(s![3, 4, 5], s!(0, 0, 5)).unwrap(), s![3, 4, 5])
    }

    #[test]
    fn reshape_with_leading_zero_with_flatten() {
        assert_eq!(&*compute_shape(s![2, 3, 5, 7], s!(2, 0, 35)).unwrap(), s![2, 3, 35])
    }

    #[test]
    fn reshape_with_trailing_zero() {
        assert_eq!(&*compute_shape(s![3, 4, 5], s!(3, -1, 0)).unwrap(), s![3, 4, 5])
    }

    #[test]
    fn reshape_bug_1() {
        assert_eq!(&*compute_shape(s![TDim::s(), 1, 2, 128], s!(0, 0, -1)).unwrap(), s![TDim::s(), 1, 256])
    }
}
